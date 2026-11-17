# 使用流程（方案）

> 状态：已实现。具体行为以 [CLI.md](CLI.md) / [STATE.md](STATE.md) / [BUILD.md](BUILD.md)
> 为准；本文保留判定矩阵与交互规则的设计说明。

目标：把 `install` / `add` / `remove` 的职责分开，让“装好之后加扩展”变成一条命令，
并且所有破坏性操作都先检查现场、可跳过、可回退。

## 1. 命令语义

| 命令 | 管什么 | 是否改方案 |
| --- | --- | --- |
| `install <产品>@<系列>` | 确保该系列当前目标版本存在（首次 / 继续中断的安装） | 否 |
| `install <产品>@<补丁>` | 明确要这个补丁（升级 / 降级） | 否 |
| `install <产品>@<系列> '组件列表'` | 用给定列表**定义方案**并构建（迁移 / 重建） | 替换 |
| `update <产品>@<系列>` | 跟随 catalog 最新补丁 | 否 |
| `add <产品>@<系列> <组件 spec>` | 加组件 → 立即增量构建 | 是 |
| `remove <产品>@<系列> <组件>` | 删组件 → 删产物 / 必要时重编 | 是 |
| `scheme <产品>@<系列>` | 导出可复制的重建命令 | 否 |

```bash
phprt install php@8.5                       # 首次安装，按 bundle 默认集
phprt add php@8.5 'swoole'                  # pecl 扩展：默认 shared，只编 .so
phprt add php@8.5 'gd=mode:shared'          # 内置扩展：必须显式写 mode
phprt remove php@8.5 swoole                 # 删 ini + .so，不重编
phprt remove php@8.5 gd                     # 静态：确认后重编 PHP
phprt scheme php@8.5                        # 打印一条 install 命令
```

## 2. install

### 2.1 判定矩阵

| 现状 | 命令 | 行为 |
| --- | --- | --- |
| 没装过 | `install php@8.5` | 按 catalog 里的目标补丁安装 |
| 已装 8.5.12、完整，catalog 解析到 8.5.13 | `install php@8.5` | **拒绝**：提示已存在旧版本，要求显式写补丁号（见 2.2） |
| 已装 8.5.12、完整 | `install php@8.5.12` | **拒绝**：说明编译配置一致 / 不一致 |
| 已装 8.5.12、完整 | `install php@8.5.12 --force` | 下载、解压成功后旧树改名 `<产品>-<补丁>-backup-<日期>`，再全新安装 |
| 已装 8.5.12（无完成标记） | 任意 | 视为中断，继续安装 |
| 已装 8.5.12、完整 | `install php@8.5.13` | 升级：构建新树 → 成功后才写 state |
| 已装 8.5.13 | `install php@8.5.12` | 降级：旧树完整就切回，否则重建 |
| state 说装过、目标补丁的树不存在 | 任意 | 直接装（树被删过，没有要保护的东西） |

### 2.2 拒绝重复安装

```text
$ phprt install php@8.5.12
error: php 8.5.12 已安装（.phprt/php-8.5.12），编译配置与本次方案一致
  要重新安装，任选其一：
    phprt install php@8.5.12 --force     # 现有树改名成 .phprt/php-8.5.12-backup-<日期>，然后全新安装
    rm -rf .phprt/php-8.5.12             # 确认不再需要这个版本，删掉后再装
```

```text
$ phprt install php@8.5            # 已装 8.5.12
error: 已存在旧版本 php 8.5.12（.phprt/php-8.5.12），本次解析到 8.5.13
  要装 8.5.13，请显式写补丁号：
    phprt install php@8.5.13
  或者交给 --force：直接装 8.5.13，不动 8.5.12 这个版本
    phprt install php@8.5 --force
```

方案指纹不同时说明“编译配置与本次方案不一致”，并把已装方案和本次方案两条命令都打出来。
catalog 里的补丁号只代表“本次解析到的目标”，文案里不出现“最新/过期”。

### 2.3 完成标记

```text
.phprt/php-8.5.12/.phprt-installed   # 最后一步原子写入：version + 方案哈希
```

目录存在只说明“试过”，标记存在才说明“装完”。两者结合判断中断与残留：

| 目录 | 标记 | `installed_version` | 结论 |
| --- | --- | --- | --- |
| 有 | 有 | 8.5.12 | 完整 |
| 有 | 无 | 8.5.12 | 上次失败/中断，可继续 |
| 有 | 无 | 空 | 首次安装没收尾 |
| 无 | 无 | 8.5.12 | 树被删过，直接装 |
| 有 | 有 | 其他版本 | 旧版本残留，可切回或清理 |

## 3. add

### 3.1 编译方式

- 组件的 `modes` 是契约，只能选其中列出的方式；违反时报支持列表。
- 未写 `mode` 时：
  - **bundled（内置）→ 报错，要求显式确认**，不静默取静态；
  - external（pecl 等）→ 取产品 profile 的 `defaults["external"]`（shared）。
- bundle 的 `[default_components].mode` 与 profile `defaults` 只约束 `install`，不替 `add` 做决定。

```text
$ phprt add php@8.5 gd
error: gd 是内置组件，add 时必须显式选择编译方式：
  phprt add php@8.5 'gd=mode:static'   # 编进产品，需要重编
  phprt add php@8.5 'gd=mode:shared'   # 编成动态扩展，不重编产品

$ phprt add php@8.5 'xdebug=mode:static'
error: component:xdebug 契约不支持 mode:static（支持：shared）
```

- 已在方案里且方式相同 → “已在方案里”，退出 0；
- 已在方案里但方式不同 → 拒绝，提示先 `remove` 再 `add`。

### 3.2 磁盘一致性检查

| 现场 | 行为 |
| --- | --- |
| 不在方案，目标 ini 不存在 | 正常添加并编译 |
| 不在方案，目标 ini 已存在 | **拒绝**：可能是用户文件，不覆盖 |
| 在方案，产物 + ini + `php -m` 一致 | 提示已安装，退出 0 |
| 在方案，缺 `.so` / ini / 模块未加载 | **拒绝**，列出差异 |

```text
$ phprt add php@8.5 swoole
swoole 已在方案里（shared），磁盘不一致：
  缺少 etc/conf.d/27-swoole.ini
  缺少 …/swoole.so
用 --force 重新编译并登记，或 phprt remove php@8.5 swoole
```

## 4. remove

### 4.1 检查与交互

| 现场 | 默认行为 |
| --- | --- |
| ini 在且未被修改，`.so` 在 | 备份 ini 后删除，打印删了哪些文件 |
| ini 被修改过（与安装时记录的哈希不同） | **拒绝**；TTY 下给差异摘要并问 `y/N` |
| ini 缺失 / `.so` 缺失 | **拒绝**，列出缺失项；`--force` 只更新方案 |
| 组件是 static（fold） | 提示重编 PHP 与预计时间，TTY 确认后执行 |

- 非 TTY 环境无法确认 → 一律拒绝并提示 `--force`；
- `--force` 跳过检查与确认，但仍执行 ini 自动备份（见第 5 节）。

### 4.2 静态删除：提示用户自行备份已装版本

静态扩展编进 PHP，删除需要原地重编，会覆盖 `.phprt/php-8.5.12/`：

```text
$ phprt remove php@8.5 gd
gd（static）需要和 php 一起重新 configure/make（约 3 分钟），并覆盖 .phprt/php-8.5.12/ 里的文件
建议先自行备份：
  cp -a .phprt/php-8.5.12 .phprt/php-8.5.12.bak-$(date +%F-%H%M)
继续？[y/N]
```

- 只提示，不代做备份，不提供 `--backup`；
- 同一提示适用于所有会重编已装版本的操作：`add 'xxx=mode:static'`、`install <补丁> --force`；
- 动态删除（只删 ini 和 `.so`）不提示备份。

## 5. ini 自动备份

对所有会改动 ini 的操作，默认先备份，不记录清单、不写 state。

| 项 | 规则 |
| --- | --- |
| 目录 | 由产品 profile 的 `backup(relative_dir, name)` 声明；`relative_dir` 相对工具目录（默认 `.phprt`，构建后的安装树所在），PHP 声明为 `backup` |
| 文件名 | `<日期时间>_<组件名>.ini`，如 `20261117-003012_swoole.ini`；重名加 `-2` |
| 范围 | 只备份 ini；不备份 `.so`、不备份已装版本 |
| 时机 | remove 删除 ini 前；add 覆盖已有 ini 前；以后修改 php.ini 等同理 |
| 跳过 | 内容与将要写入的一致时不备份；ini 不存在时不备份 |
| 失败 | 备份失败则终止本次操作，并提示可用 `--no-backup-php-ext-ini` |
| 参数 | `--no-backup-php-ext-ini` 跳过本次备份（`--force` 不隐含此项） |

```text
$ phprt remove php@8.5 swoole
已移除 swoole（shared），删掉 2 个文件：
  lib/php/extensions/no-debug-non-zts-20250925/swoole.so
  etc/conf.d/27-swoole.ini
已移除组件 swoole
```

`.phprt/backup/` 没有 `state.json`，`list` 不会把它当成安装产品。

## 6. scheme：导出与导入

```text
$ phprt scheme php@8.5
phprt install php@8.5 'curl=mode:shared,gd=mode:shared,swoole,zip=dep:libzip[encryption]'
```

导出内容：

- 完整组件列表（含当时生效的默认组件）；
- `mode`、用户显式的 `feature` / `dep`、用户钉的版本线；
- **不含**补丁号、解析出的依赖线交集、feature 并集、configure 参数。

导入规则：

- `install <产品>@<系列> '列表'` 的列表是**完整方案**，直接替换，不与 bundle 默认集合并；
- 列表里有 recipe 已删除的组件 → 报错并指出名字；
- 同一份列表在别的机器上得到相同方案、当时的补丁（补丁跟随 catalog，不锁）；
- 方案列表不能和 `--component` / `--disable-default-component` 一起用。

`scheme` 只打印一条命令，不提供差异或展开开关；该系列没有 state 时直接报
「当前没有构建过 <产品> <系列>」，不凭空展示默认集。

## 7. state 模型

state 只保存两类数据，解析结果每次重算：

```text
方案（输入，可手改，需校验）  components + 每项的 mode/features/deps/版本线 + bindings
安装事实（工具写）            installed.version + 每项的 mode/artifacts/ini/ini_hash
```

草案：

```json
{
  "version": 4,
  "product": "php",
  "series": "8.5",
  "scheme": {
    "components": {
      "gd":     { "mode": "static", "features": ["jpeg"], "deps": { "jpeg": ["simd"] } },
      "swoole": { "mode": "shared" }
    },
    "bindings": { "openssl": "3.5" }
  },
  "installed": {
    "version": "8.5.12",
    "components": {
      "swoole": {
        "mode": "shared",
        "artifacts": ["lib/php/extensions/…/swoole.so"],
        "ini": "etc/conf.d/27-swoole.ini",
        "ini_hash": "sha256:…"
      }
    }
  }
}
```

- 不保存 `deps.*` 的解析结果、组件补丁号、configure 参数；
- 手改只允许改 `scheme`，加载时校验组件名 / mode / feature / dep；
- `add` / `remove` 一律从 `scheme` 重新求解，不把 state 里的派生值当输入；
- schema 升级时旧字段直接丢弃，不迁移。

## 8. `--force` 的边界

- 只跳过一致性检查、覆盖确认、重复安装限制；
- `install --force` 在“目标补丁已经装过”时，等下载和解压成功后把现有树改名
  `<产品>-<补丁>-backup-<日期>`（重名加 `-2`），再全新安装；补丁不同时不改名；
- 不忽略下载失败、编译错误、校验失败；
- 不替代 ini 备份（备份仍需 `--no-backup-php-ext-ini` 才跳过）；
- 非 TTY 下所有需要确认的操作都必须靠它放行。

## 9. 退出码

| 情况 | 退出码 |
| --- | --- |
| 成功 / 无变化 | 0 |
| 拒绝重复安装、检查不通过、用户取消 | 1 |
| 编译/下载失败 | 1 |
| Ctrl-C | 130 |

## 10. 验收

```bash
phprt install php@8.5.12                  # 第二次：拒绝
phprt install php@8.5.12 --force          # 允许重装
phprt install php@8.5                     # catalog 更新后：拒绝并提示 update

phprt add php@8.5 gd                      # 拒绝：内置扩展必须写 mode
phprt add php@8.5 'gd=mode:shared'        # 只编 gd，PHP 不动
phprt add php@8.5 'gd=mode=static'        # 提示备份已装版本，确认后重编
phprt add php@8.5 swoole                  # pecl：默认 shared
phprt remove php@8.5 swoole               # 备份 ini → 删 ini + .so
phprt remove php@8.5 swoole --no-backup-php-ext-ini
phprt remove php@8.5 gd                   # 静态：确认 + 重编

phprt scheme php@8.5                      # 输出可直接执行的 install 命令
phprt install php@8.5 '…'                 # 按导出命令重建
```

## 11. 落地清单

0. **前置**：构建签名不要包含调用者环境（`PATH` / `CFLAGS` / `PKG_CONFIG_PATH` 等），
   否则换 shell 或 CI 就会全量重编，“add shared 不重编 PHP”的承诺不成立。
1. `src/main.rs`：`install` 增加版本判定与拒绝逻辑；`add` / `remove` 改为“改方案后立即构建”；新增 `scheme` 子命令；`--force` / `--no-backup-php-ext-ini`。
2. `src/state.rs`：schema 4（`scheme` + `installed`），删除 `deps` 解析字段与 `line/version` 派生字段。
3. `src/build.rs`：写完成标记；`remove` 删除产物与 ini（按 mode 的 `artifacts`）；静态删除走重编。
4. ini 写入前比较内容并在需要时备份到 `.phprt/backup/`。
5. 交互：TTY 确认、非 TTY 拒绝；`--force` 放行。
6. 文档：实现后并入 CLI.md / STATE.md / BUILD.md，删除本文件；`AGENTS.md` 验收补第 10 节用例。
