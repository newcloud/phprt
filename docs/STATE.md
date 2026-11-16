# 状态文件

状态文件只记录**当前构建方案和已编译产物的状态**，不锁定版本。补丁号仍然从
`recipes/catalog.toml` 取。

## 1. 路径

```text
.phprt/<产品>-<系列>/state.json
```

按产品系列存放：php 8.5 → `.phprt/php-8.5/`，nginx 1.29 → `.phprt/nginx-1.29/`。
补丁从 8.5.12 升到 8.5.13 时，方案和记录不会丢；安装树仍然是
`.phprt/php-8.5.13/`。不同产品、不同系列的 state 互不影响。

## 2. 格式

状态 schema 是版本 3：

```json
{
  "version": 3,
  "active": true,
  "product": "php",
  "series": "8.5",
  "installed_version": "8.5.12",
  "bindings": {
    "swoole": "6.2"
  },
  "components": {
    "swoole": {
      "line": "6.2",
      "version": "6.2.5",
      "mode": "shared",
      "installed": true,
      "deps": { "libcurl": ["ssl"] },
      "features": []
    },
    "openssl": {
      "line": "",
      "version": "8.5.12",
      "mode": "static",
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
| `version` | schema 版本，固定 3；旧文件直接报错，不自动迁移 |
| `active` | 是否已经写过方案。写过之后 `components` 就是权威列表，即使为空 |
| `product` / `series` | 这份状态所属的产品和系列 |
| `installed_version` | 最近一次成功安装的产品补丁版本，仅记录 |
| `bindings` | 用户显式指定的版本线；bundle 解析出来的默认线不写在这里 |
| `components.<名字>.line` | 实际使用的版本线，如 6.2；bundled 组件为空 |
| `components.<名字>.version` | 实际补丁，如 6.2.5；只记录，不锁定 |
| `components.<名字>.mode` | 当前挂载方式，如 static / shared / dynamic |
| `components.<名字>.installed` | 是否已经编译安装并通过产品启动校验 |
| `components.<名字>.deps` | 依赖名 → 转给它的 feature |
| `components.<名字>.features` | 组件自己的 feature |
| `deps.<名字>.line` | 依赖实际使用的版本线 |
| `deps.<名字>.version` | 依赖实际补丁 |
| `deps.<名字>.features` | 当前构建使用的 feature 并集 |

`deps` 里也会记录构建期工具（如 `autoconf`）：它们参与构建顺序，但不参与链接。

`bindings` 是“保留的显式选择”。例如：

```bash
phprt install php@8.5 --component swoole=version:6.0
```

之后不带 `version:` 再执行 install，仍会停在 6.0，因为 `bindings.swoole = "6.0"`。
bundle 只影响没有显式选择的组件。

`bindings` 的键是 catalog 组件名：有独立版本线的组件（如 `swoole`）或依赖库（如
`openssl`）。内置组件没有自己的版本线，不会出现在这里；`bindings.openssl` 一定指
OpenSSL 库。

## 3. 选线优先级

```text
CLI 显式 --component ...=version:线 / --dep NAME=线
  > state.bindings
  > bundle 默认线（组件写顶层，依赖库写 [deps]）
  > 组件只有一条线时直接用
```

如果某个依赖被多个组件或产品约束，取所有约束的交集：

```text
allowed = catalog 里该依赖的线
        ∩ 产品 LINES[系列] 允许的线
        ∩ 每个已选组件允许的线
```

- 显式 pin 不在交集里 → 报错。
- bundle 默认线不在交集里、但交集只有一个 → 自动改用并打印提示。
- 交集有多个或为空 → 报错并列出候选。

## 4. 读写时机

| 命令 | 读 | 写 | installed |
| --- | --- | --- | --- |
| `plan` / `graph` | 是 | 否 | 不变 |
| `list` | 是 | 否 | 不变 |
| `install` / `update` | 是 | 是 | 方案内全部设为 true |
| `install` / `update --downloads` | 是 | 否 | 不变 |
| `install --component` | 是 | 是 | 只写状态里已有的方案，不保留临时组件 |
| `add` | 是 | 是 | 新组件 false |
| `remove` | 是 | 是 | 删除条目 |

第一次写状态时从当前产品系列的 `bundle.default_components` 补种；php 8.5 会写入
默认内置扩展（openssl、curl、libxml、zlib、sqlite3 等）；nginx 默认不装第三方模块。
补种只发生一次；之后 `default_components` 变化不影响已有 state。

`add` / `--component` 首次把组件写进 state 时，会把
`bundle.component_presets.<组件>` 的 features/deps 一起 materialize；之后即使 preset
变化，已有 state 也不改。如果 state 里该组件的 features 已经为空，表示用户显式清空过，
不再重新套 preset。

`update` 只在 state 记录的线内重新从 catalog 取补丁，不自动换线。catalog 删掉了
这条线时直接报错，不静默迁移。

## 5. feature 规则

- `add` / `install` / `update`：`deps.features` 取并集，只增不减。
- 组件自己的 `features` 以显式选择为准：CLI 写了 `feature:[...]`（包括 `feature:[]`）
  就覆盖 state 里的值，没写就沿用 state；没有 state 时用 `[component_presets]`。
- `remove`：按剩余组件的 dep 重新计算，允许收窄。
- 状态里记录了每个组件的 `deps` / `features`，所以 `add` 不会冲掉其他组件之前的选择。
- 状态里有、recipe 里没有的组件或 feature → 报错，要求手工处理。
- feature 的 `requires` 不满足（产品系列、依赖线或宿主机命令变了）→ 报错并提示改 state、
  装缺少的命令或换依赖线。

## 6. 写入边界

`install` / `update` 做：

1. 解析版本线并查 catalog。
2. 求解。
3. 下载源码到 `.phprt/downloads/`（缓存命中跳过）。
4. 按构建顺序解压到 `build/<名字>/<补丁>/`。
5. 编译安装到 `.phprt/<产品>-<补丁>/`。
6. 按产品 profile 做运行时整理和校验；PHP 再写 `myphp-<系列>`（只有一个 PHP 系列时
   额外写 `myphp`）。
7. 更新 `state.json`。

`add` 只把组件写进方案（待安装）；`remove` 改状态并删掉该组件的 `conf.d` ini（PHP），
让它在下次启动时不再加载。全新项目上的 `add` / `remove` 会先把当前系列的默认集
seed 进 state。

`--disable-default-component` 从方案里扣除默认组件：首次 install 时影响
`default_components`，已有 state 时从 `state.components` 移除。它不修改 bundle，也不做
依赖展开。

`--downloads` 只做第 1～3 步，不解压、不编译、不写状态；下载、解压或编译失败时
命令直接退出，也不写状态文件。
