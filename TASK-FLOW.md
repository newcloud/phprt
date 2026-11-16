# 任务：实现 phprt v3 的 install / add / remove / scheme 使用流程

> 这是一份交给开发 agent 的任务书。规格来源是 `docs/FLOW.md`，本文件负责交代
> 上下文、边界、实施顺序和验收方式。两者冲突时以 `docs/FLOW.md` 为准。

## 1. 项目与环境

- 仓库：`/home/z/testphp/phprtv3`（Rust workspace + Starlark recipe）
- 语言/工具链：`cargo`（edition 2024）
- 实机验收目录：`/home/z/testphp/v3test`（已装 PHP 8.5，可直接 add/remove 测试）
- **不要动** `/home/z/testphp/blog`（那是用户的实际项目）

在项目目录里运行 phprt 时必须显式指定 recipe 和项目根，例如：

```bash
P=/home/z/testphp/phprtv3/target/debug/phprt
R=/home/z/testphp/phprtv3/recipes
$P --recipes $R --root /home/z/testphp/v3test list php@8.5
```

## 2. 开工前必读

1. `AGENTS.md` —— 开发约定，重点是：
   - 文档必须和代码同步；
   - 不开发没明确要求的使用方式；
   - 验收命令在最后一节。
2. `docs/FLOW.md` —— **本次要实现的全部规则**（命令语义、判定矩阵、交互、备份、scheme、state 模型）。
3. 待同步的文档：`docs/CLI.md`、`docs/STATE.md`、`docs/BUILD.md`、`docs/CONTRACT.md`、`docs/DESIGN.md`。
4. 关键源码：
   - `src/main.rs`：`Cmd::Install` / `Cmd::Add` / `Cmd::Remove` 的现有实现
   - `src/state.rs`：`ProjectState` / `ComponentState` / `add_component_with_preset`
   - `src/build.rs`：`run()`、`disable_component()`、`build_env()`、`signature()`
   - `src/recipe.rs`：`modes` 契约、`default_mode`、profile `defaults`
   - `crates/build/src/lib.rs`：签名跳过（`.phprt-sig`）与产物检查

## 3. 工作区现状（重要）

- 工作区有**未提交**的改动：
  - `src/build.rs`：签名重构（`dep_sigs` / `source_sig`），**当前 `cargo build` 编不过**；
  - `src/recipe.rs`：新增测试；
  - `crates/download/**`：已完成的“连接被重置重试 3 次”，**已经验收，不要改动或回退**；
  - `docs/DOWNLOAD.md`、`docs/DESIGN.md` 等：对应文档改动。
- 动手前先跑 `cargo build` 确认。如果因为 `src/build.rs` 的 WIP 编不过，**先停下来问用户**
  怎么处理（补完 / 暂存 / 由用户自己处理），不要 revert、不要删除别人的改动。
- `target/debug/phprt` 是旧二进制，不含本次要实现的改动；需要重新编译。

## 4. 要实现的规格（摘要，细则见 docs/FLOW.md）

### 4.1 命令语义

| 命令 | 行为 |
| --- | --- |
| `install <产品>@<系列>` | 确保该系列当前目标版本存在；中断的安装可继续 |
| `install <产品>@<补丁>` | 指定补丁（升级/降级） |
| `install <产品>@<系列> '组件列表'` | 用列表**定义方案**（替换），然后构建 |
| `update <产品>@<系列>` | 跟随 catalog 最新补丁 |
| `add <产品>@<系列> <组件 spec>` | 改方案 → **立即增量构建** |
| `remove <产品>@<系列> <组件>` | 删组件 → 删产物 / 必要时重编 |
| `scheme <产品>@<系列>` | 打印一条可执行的重建命令 |

组件列表用现有 spec 语法，逗号分隔多个；`[...]` 内的逗号不切分。

### 4.2 install 判定

- 目标版本已完整安装（完成标记存在 + `state.installed.version` 一致）→ **拒绝**：
  ```text
  error: php 8.5.12 已安装且方案一致，不重复安装
    重装/修复: phprt install php@8.5.12 --force
    换版本:    phprt update php@8.5
  ```
- `install php@8.5` 且 catalog 有新补丁 → 拒绝并提示 `update` / 显式版本。
- 目录存在但没有 `.phprt/<产品>-<版本>/.phprt-installed` → 视为中断，允许继续。
- 完成标记在整条流水线（含 `php -m` 校验）成功后**最后一步原子写入**。

### 4.3 add 的编译方式

- 组件 `modes` 是契约；不在列表里的 mode 报错并列出支持项（已有行为，保留）。
- 未写 `mode` 时：
  - **bundled（内置）→ 报错**，必须显式确认，不静默取 static：
    ```text
    error: gd 是 PHP 内置扩展，add 时必须显式选择编译方式：
      phprt add php@8.5 'gd=mode:shared'   # 编成动态扩展，不重编 PHP
      phprt add php@8.5 'gd=mode=static'   # 编进 PHP，需要重编（约 3 分钟）
    ```
  - external（pecl 等）→ 用 profile `defaults["external"]`（shared）。
- 已在方案里：mode 相同 → 提示已存在；mode 不同 → 拒绝，提示先 `remove`。

### 4.4 add / remove 的磁盘一致性检查

删除/覆盖前检查“state 记录的事实”与磁盘：

| 现场 | 行为 |
| --- | --- |
| add：目标 ini 已存在但方案里没有该组件 | 拒绝（可能是用户文件），`--force` 才覆盖 |
| add：方案里有、缺 `.so`/ini/模块未加载 | 拒绝，列出差异，`--force` 重编 |
| remove：ini 与安装时哈希一致、`.so` 存在 | 备份 ini 后删除 |
| remove：ini 被改过 | 拒绝；TTY 给差异摘要问 `y/N`；非 TTY 拒绝 |
| remove：ini 或 `.so` 缺失 | 拒绝并列出缺失项；`--force` 只更新方案 |
| remove：static（fold） | 提示将重编 PHP 与耗时，确认后执行 |

- 非 TTY 一律拒绝并提示 `--force`，不要阻塞等待输入。
- 静态删除要打印“建议自行备份安装树”的提示和 `cp -a .phprt/php-8.5.12 .phprt/php-8.5.12.bak-$(date +%F-%H%M)`；
  **不代做备份，不提供 `--backup`**。

### 4.5 ini 自动备份（默认开启）

- 目录：`.phprt/backup/`；文件名：`<日期时间>_<组件名>.ini`（如 `20261117-003012_swoole.ini`，重名加 `-2`）。
- 只在会改动 ini 时备份（remove 删除前、add 覆盖前）；内容无变化或 ini 不存在时不备份。
- 备份失败 → 终止操作并提示可加 `--no-ini-backup`。
- 参数 `--no-ini-backup` 跳过本次备份；`--force` **不**隐含跳过备份。
- `.phprt/backup/` 不写 state，`list` 不应把它当成安装产品。

### 4.6 scheme

```text
$ phprt scheme php@8.5
phprt install php@8.5 'curl=mode=shared,gd=mode=shared,swoole,zip=dep:libzip[encryption]'
```

- 内容：完整组件列表（含当时生效的默认组件）+ mode + 用户显式 feature/dep/版本线。
- **不含**补丁号、解析出的依赖线交集、feature 并集、configure 参数。
- `install <产品>@<系列> '列表'` 按完整方案**替换**，不与 bundle 默认集合并；
  列表里有已删除的组件 → 报错指出名字。

### 4.7 state 模型（schema 4）

- 只存两类：`scheme`（用户输入，可手改、需校验）和 `installed`（工具写的事实）。
- 删掉现在的 `deps` 解析块、`components.*.line/version` 派生字段。
- `installed.components.<名>` 记 `mode`、`artifacts`、`ini`、`ini_hash`。
- 旧 schema（3）不自动迁移：读到旧文件直接报错，要求重新安装（与现文档一致）。
- `add` / `remove` 一律从 `scheme` 重新求解，不得把 state 里的派生值当输入。

### 4.8 --force 边界

只跳过一致性检查、覆盖确认、重复安装限制；不忽略下载/编译/校验错误。

## 5. 实施顺序（每阶段结束停下汇报，等确认再继续）

- **阶段 0（只调查，不改代码）**
  - 跑 `cargo build`，报告 WIP 编不过的原因和处理建议；
  - 确认 `build_env()` + `signature()` 是否把调用者环境（`PATH`/`CFLAGS`/`PKG_CONFIG_PATH` 等）
    算进签名。如果是，说明“add shared 不重编 PHP”会失效，提出修改方案等用户确认。
- **阶段 1**：完成标记 + install 判定（4.2）。
- **阶段 2**：add/remove 的检查、交互、`--force`、静态重编（4.3、4.4）。
- **阶段 3**：ini 自动备份 + `--no-ini-backup`（4.5）。
- **阶段 4**：scheme 导出 / install 列表导入（4.6）。
- **阶段 5**：state schema 4（4.7）+ 文档同步 + 删除 `docs/FLOW.md` 与其在
  `docs/DESIGN.md` 的索引行 + `AGENTS.md` 验收补用例。

## 6. 硬性约束

- 只实现本任务书和 `docs/FLOW.md` 写明的行为；不要做 lock 文件、`--backup`、
  IPv4/IPv6 切换、批量 add 语法等计划外功能。
- 不要顺手重构无关代码；不要改 `crates/download/**`（已完成并验收）。
- 保持现有风格：错误信息中文、`bail!`/`anyhow`、文档与输出一致。
- 交互只在 TTY 下等待输入；非 TTY 一律拒绝 + 提示 `--force`。

## 7. 测试与验收

每个阶段至少跑：

```bash
cargo fmt -p <改动的 crate>
cargo test -p <改动的 crate>
cargo clippy -p <改动的 crate> --all-targets
```

全部完成后（WIP 修复后）：

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets
```

实机验收（`v3test`，逐条贴真实输出）：

```bash
P="/home/z/testphp/phprtv3/target/debug/phprt --recipes /home/z/testphp/phprtv3/recipes --root /home/z/testphp/v3test"

$P install php@8.5.12                  # 第二次：必须拒绝
$P install php@8.5.12 --force          # 允许重装
$P add php@8.5 gd                      # 必须拒绝：内置扩展要显式 mode
$P add php@8.5 'gd=mode=shared'        # 只编 gd，PHP 不动
$P add php@8.5 swoole                  # pecl 默认 shared
$P remove php@8.5 swoole               # 生成 .phprt/backup/*_swoole.ini，删 ini + .so
$P remove php@8.5 swoole --no-ini-backup
$P scheme php@8.5                      # 输出可直接执行；粘贴执行后方案一致
```

自动化测试至少要覆盖：重复安装拒绝、完成标记判定、内置扩展未指定 mode 报错、
ini 备份命名与跳过、remove 检测 ini 被修改、scheme 导出可重新导入。

## 8. 汇报格式

每个阶段结束，给出：

1. 改了哪些文件（路径 + 一句话说明）；
2. 关键行为的**真实命令输出**（不是描述）；
3. 新增/修改的测试名与结果；
4. 未完成项、已知风险、需要用户决定的点。

## 9. 完成定义（DoD）

- [ ] `docs/FLOW.md` 第 10 节验收命令全部符合预期；
- [ ] `cargo test --workspace`、`cargo clippy --workspace --all-targets` 通过；
- [ ] `docs/CLI.md` / `docs/STATE.md` / `docs/BUILD.md` 已写入真实行为；
- [ ] `docs/FLOW.md` 已删除，`docs/DESIGN.md` 索引行已移除；
- [ ] `AGENTS.md` 第 5 节验收补充了新用例；
- [ ] 没有改动无关文件（`git diff` 可逐条对应到本任务）。
