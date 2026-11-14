# 状态文件

状态文件只记录**当前构建方案和已编译产物的状态**，不锁定版本。补丁号仍然从
`recipes/catalog.toml` 取。

## 1. 路径

```text
.phprt/php-<系列>/state.json
```

按系列存放，`8.5.12` → `php-8.5`。补丁从 8.5.12 升到 8.5.13 时，方案和记录不会丢；
安装树仍然是 `.phprt/php-8.5.13/`。

## 2. 格式

```json
{
  "version": 2,
  "active": true,
  "php": "8.5.12",
  "bindings": {
    "swoole": "6.2"
  },
  "ext": {
    "swoole": {
      "line": "6.2",
      "version": "6.2.5",
      "link": "shared",
      "installed": true,
      "deps": { "libcurl": ["ssl"] },
      "features": []
    },
    "openssl": {
      "line": "",
      "version": "8.5.12",
      "link": "static",
      "installed": true,
      "deps": {},
      "features": ["argon2"]
    }
  },
  "deps": {
    "openssl": {
      "line": "3.5",
      "version": "3.5.9",
      "features": []
    }
  }
}
```

| 字段 | 说明 |
| --- | --- |
| `active` | 是否已经写过方案。写过之后 `ext` 就是权威列表，即使为空 |
| `php` | 最近一次求解的 PHP 补丁版本，仅记录 |
| `bindings` | 用户显式指定的版本线；bundle 解析出来的默认线不写在这里 |
| `ext.<名字>.line` | 实际使用的版本线，如 6.2 |
| `ext.<名字>.version` | 实际补丁，如 6.2.5；只记录，不锁定 |
| `ext.<名字>.link` | 当前链接方式 |
| `ext.<名字>.installed` | 是否已经编译安装并通过启动校验 |
| `ext.<名字>.deps` | 依赖名 → 转给它的 feature |
| `ext.<名字>.features` | 扩展自己的 feature |
| `deps.<名字>.line` | 依赖实际使用的版本线 |
| `deps.<名字>.version` | 依赖实际补丁 |
| `deps.<名字>.features` | 当前构建使用的 feature 并集 |

`deps` 里也会记录构建期工具（如 `autoconf`）：它们参与构建顺序，但不参与链接。

`bindings` 是“保留的显式选择”。例如：

```bash
phprt install 8.5 --ext swoole=version:6.0
```

之后不带 `version:` 再执行 install，仍会停在 6.0，因为 `bindings.swoole = "6.0"`。
bundle 只影响没有显式选择的组件。

`bindings` 的键是 catalog 组件名：有独立版本线的扩展（如 `swoole`）或依赖库（如 `openssl`）。
内置扩展没有自己的版本线，不会出现在这里；`bindings.openssl` 一定指 OpenSSL 库。

## 3. 选线优先级

```text
CLI 显式 --ext ...=version:线 / --dep NAME=线
  > state.bindings
  > bundle 默认线（扩展写顶层，依赖库写 [deps]）
  > 组件只有一条线时直接用
```

如果某个依赖被多个扩展约束，取所有约束的交集：

```text
allowed = catalog 里该组件的线
        ∩ 每个已选扩展允许的线
```

- 显式 pin 不在交集里 → 报错。
- bundle 默认线不在交集里、但交集只有一个 → 自动改用并打印提示。
- 交集有多个或为空 → 报错并列出候选。

## 4. 读写时机

| 命令 | 读 | 写 | installed |
| --- | --- | --- | --- |
| `plan` / `graph` | 是 | 否 | 不变 |
| `list` / `ext list` | 是 | 否 | 不变 |
| `install` / `update` | 是 | 是 | 全部设为 true |
| `install` / `update --downloads` | 是 | 否 | 不变 |
| `ext add` | 是 | 是 | 新扩展 false |
| `ext remove` | 是 | 是 | 删除条目 |

第一次写状态时从当前 PHP 系列的 `bundle.default_exts` 补种；8.5 会写入
`openssl`、`curl`（curl 带 ssl）和 `libxml`；gd 不在默认集里，需要 `--ext` 显式选择。
其他系列暂时只记录 PHP。补种只发生一次；之后 `default_exts` 变化不影响已有 state。

`--ext gd` / `ext add gd` 首次把扩展写进 state 时，会把 `bundle.ext_presets.gd` 的
features/deps 一起 materialize；之后即使 preset 变化，已有 state 也不改。
如果 state 里该扩展的 features 已经为空，表示用户显式清空过，不再重新套 preset。

`update` 只在 state 记录的线内重新从 catalog 取补丁，不自动换线。catalog 删掉了
这条线时直接报错，不静默迁移。

## 5. feature 规则

- `ext add` / `install` / `update`：`deps.features` 取并集，只增不减。
- 扩展自己的 `features` 以显式选择为准：CLI 写了 `feature:[...]`（包括 `feature:[]`）
  就覆盖 state 里的值，没写就沿用 state；没有 state 时用 `[ext_presets]`。
- `ext remove`：按剩余扩展的 dep 重新计算，允许收窄。
- 状态里记录了每个扩展的 `deps` / `features`，所以 `ext add` 不会冲掉其他扩展之前的选择。
- 状态里有、recipe 里没有的扩展或 feature → 报错，要求手工处理。
- feature 的 `requires` 不满足（PHP 系列、依赖线或宿主机命令变了）→ 报错并提示改 state、
  装缺少的命令或换依赖线。
- 旧版本 state 里的 `sub` 字段会被当作 `deps` 读取，但里面的 key 需要手工迁移
  （`sub: curl` 已改名为 `dep: libcurl`）。

## 6. 写入边界

`install` / `update` 做：

1. 解析版本线并查 catalog。
2. 求解。
3. 下载源码到 `.phprt/downloads/`（缓存命中跳过）。
4. 按构建顺序解压到 `build/<组件>/<补丁>/`。
5. 编译安装到 `.phprt/php-<补丁>/`。
6. 校验 `php -v` / `php -m`，写 `myphp` 和 `.phprt/current`。
7. 更新 `state.json`。

`ext add` 只把扩展写进方案（待安装）；`ext remove` 改状态并删掉该扩展的
`conf.d` ini，让它在下次启动时不再加载。全新项目上的 `ext add/remove`
会先把当前系列的默认集 seed 进 state。

`--disable-default-ext` 从方案里扣除默认扩展：首次 install 时影响 `default_exts`，
已有 state 时从 `state.ext` 移除。它不修改 bundle，也不做依赖展开。

`--downloads` 只做第 1～3 步，不解压、不编译、不写状态；下载、解压或编译失败时
命令直接退出，也不写状态文件。
