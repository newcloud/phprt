# catalog 数据模型与更新

状态：已实现。维护者的全量流程见 [CATALOG-UPDATE.md](CATALOG-UPDATE.md)。

目标读者：维护 catalog 的人，以及需要判断 `install` / `update` / `add` / `remove` 什么时候
会联网、会改哪个文件的人。

## 摘要

- catalog 只有一份文件：`recipes/catalog.toml`，随代码仓库或发行包交给用户。没有发布
  服务器，也没有在线分片。
- 维护者用 `phprt catalog update` 做全量更新：扫全部组件、全部已有线，写回
  `recipes/catalog.toml`。
- 用户在 `install` / `update` / `add` / `remove` 里触发的是**主线补丁检查**：只查本次
  方案用到的组件主线，只在这条线内找更新的补丁；查到后写回同一份 `recipes/catalog.toml`
  （写前备份），重新算方案，再下载、构建。
- 只升补丁，不跨线、不新增线。`php 8.5` 不会因为上游出了 8.6 就变成 8.6。
- 新补丁的 sha256 下载源码包后计算；下载缓存和构建共用，同一个包下一次直接命中。
- 不联网或明确锁定版本时跳过检查，继续用本地 catalog。

## 1. 数据

`recipes/catalog.toml` 的结构：组件名 → 版本线 → 这条线的补丁数组。

```toml
[openssl]
"3.5" = [
    { version = "3.5.6", url = "https://.../openssl-3.5.6.tar.gz", sha256 = "..." },
    { version = "3.5.9", url = "https://.../openssl-3.5.9.tar.gz", sha256 = "..." },
]
```

- 一条线是一个补丁数组；默认选线内版本最大的补丁，和数组顺序无关。
- 旧补丁保留，不删除。`--lock` 和显式补丁版本按版本精确查。
- 查询规则在 `recipes/upstream/<组件名>.star`；版本比较、稳定性过滤、线归属在 Rust 侧。
- 没有第二份 catalog，没有本地覆盖层，也没有组件分片。用户的 `recipes/catalog.toml`
  就是唯一数据源。

## 2. 两类使用者

| | 维护者 | 用户 |
| --- | --- | --- |
| 入口 | `phprt catalog update` | `install` / `update` / `add` / `remove` |
| 范围 | 全部组件、每条已有线 | 本次方案用到的组件主线 |
| 产物 | `recipes/catalog.toml` + 备份 | 同一份 `recipes/catalog.toml` + 备份 |
| 参数 | `--dry-run`、`--fill-sha256`、组件过滤 | `--refresh` / `--no-refresh`、`--offline`、`--lock` |
| 失败 | 退出码 3，人工处理 | 单条线失败保留旧数据，安装继续 |

两边共用上游脚本、版本比较和写回校验，但入口、范围、交互和输出是分开的。用户命令不会
调用 `catalog update`，也不会触发全量 sweep。

## 3. 用户路径：主线补丁检查

`install` / `update` / 已安装产品上的 `add` / 需要重编的 `remove` 在下载或编译前检查一次。
只检查本次闭包里的组件主线，例如 `php 8.5`、`openssl 3.5`、`libcurl 8.23`。

```text
算方案 → 得到闭包的 (组件, 主线)
  → 这次不会下载或编译：不检查
  → 会：询问一次
    → n：用本地 catalog，继续
    → y：只查这些主线 → 有新补丁就下载源码包算 sha256 → 写回 catalog.toml（备份）
         → 重新算方案 → 下载源码 → 构建
```

### 3.1 触发

| 命令 | 什么时候检查 |
| --- | --- |
| `install` | 首次安装、方案变化、`--downloads`、`--force`，或安装树不完整 |
| `update` | 总是检查 |
| `add` | 产品已安装、加入后会立即编译时 |
| `remove` | 移除后需要重编时；默认回答 `n`，不顺便升级其它组件 |

以下情况不检查：

- `plan` / `list` / `graph` / `scheme` / `audit`：只读命令，不联网。
- 显式指定补丁，如 `install php@8.5.12`。
- `--offline`、`--lock`、`--no-refresh`，或 `ask_before_online_update = false`。
- 标准输入不是终端（CI、管道），且没有 `--refresh`。
- `add` 的产品还没安装、只记方案时。

### 3.2 范围

- 只查方案里出现的 `(组件, 主线)`；同一组件只查一次。
- 只接受属于该主线的候选版本；上游出现的新线不写入。
- 产品线取 `产品@系列` 的系列，依赖库和组件取本次绑定的线。显式
  `--component swoole=version:6.0` 只检查 `6.0` 线，不会升到 `6.2`。
- 没有 catalog 条目的节点不检查；没有 upstream 脚本的组件会报失败，保留本地数据。

### 3.3 提示

```text
准备安装 php 8.5，会用到这些组件：
  php 8.5        本地 8.5.13
  openssl 3.5    本地 3.5.9
  libcurl 8.23   本地 8.23.0

安装前是否检查上游这条主线的最新补丁？ (y/n，回车默认 y)
只查上面这些主线；有新补丁会下载源码包计算 sha256，写回 recipes/catalog.toml。
```

- `update` 会显示已装版本和本地版本，并用 `↑` / `=` / `↓` 标出差异。
- 已装过一部分时，行里显示「已装」。
- `remove` 的询问固定默认 `n`，不受 `default_answer` 影响。

### 3.4 显示

查询阶段先把主线列出来，查到哪个就切到「查询中 / 检查 <线>」，完成后逐条线输出。
有新补丁时直接从上游下载源码包、算哈希，再多一行 `已备份:` / `已写入:`：

```text
[catalog] php ✓
    8.5  8.5.12 → 8.5.13
[catalog] openssl ✓
    3.5  无更新 3.5.9

已备份: .phprt/backup/20261121-175336_catalog.toml
已写入: recipes/catalog.toml
php 8.5: 8.5.12 → 8.5.13
  url: https://www.php.net/distributions/php-8.5.13.tar.gz
openssl 3.5: 无更新 3.5.9
catalog 已更新，继续。
```

- 行格式和 `catalog update` 一样：`组件 线: 旧 → 新`、`无更新 <版本>`、`无候选`、
  `失败 <版本>`。
- 没有任何变化时不写文件，也不打印备份行。
- 非终端退化成一行一个组件，不会长时间没有输出。

### 3.5 下载和 sha256

- 新补丁必须先探测到可下载的源码包；sha256 下载后计算，才能写入 catalog。
- sha256 由用户机器计算，不依赖维护者预先填好；下载缓存是
  `<download-dir>/<组件>/`，构建阶段复用同一个文件。
- 已有补丁的 sha256 不会被重新计算；补缺失哈希仍是维护者的 `--fill-sha256`。
- 上游连接、文件名或 sha256 失败时，该条线保留本地数据并报告 `✗`，不挑更旧的补丁顶替。

### 3.6 失败、退出码和 `--refresh`

- 交互式 `y`：单条线失败保留本地数据，其它线照常更新，安装继续；不影响退出码。
- `--refresh`：跳过询问直接检查；有任何失败项时，已成功的补丁照常写入，然后按错误退出。
- `--refresh` 不能和 `--offline` / `--lock` / `--no-refresh` 一起用。
- 检查完成后整条命令重跑一遍，方案、绑定和版本都按新 catalog 重新算；同一轮不会重复检查。

### 3.7 配置

`phprt.toml`（项目根，或 `--root` 指向的目录）：

```toml
[catalog]
ask_before_online_update = true   # false = 不问、不查，直接用本地
default_answer = "y"              # 仅交互式生效
```

| 方式 | 行为 |
| --- | --- |
| 默认 | 询问一次；回车按 `default_answer` |
| `ask_before_online_update = false` | 不问、不查 |
| `--refresh` | 不问，直接查；失败按错误处理 |
| `--no-refresh` | 不问，不查 |
| `--offline` | 不联网，用本地 |
| `--lock` | 不问，按 state 记录的版本解析 |

命令行优先于配置。`--offline` 同时禁止下载和补丁检查。配置值非法（不是 true/false、
`default_answer` 不是 y/n）直接报错。

## 4. 维护者

```bash
phprt catalog update [组件...] [--fill-sha256] [--dry-run]
```

全量语义、查询规则、报告格式见 [CATALOG-UPDATE.md](CATALOG-UPDATE.md)：

- 扫全部组件、每条已有线；只升补丁，不新增线。
- sha256 下载后计算；`--fill-sha256` 只回填已有条目缺失的值。
- 写前备份，写前复检；没有变化不写文件。
- 有组件失败但其余成功时退出码 3。

## 5. 边界

- 用户路径不做全量 sweep，不处理方案之外的组件。
- 用户路径不新增版本线，也不改 recipe、bundle、state。
- catalog 是共享数据文件；两个命令同时写会互相覆盖。需要并发写时由调用方自己串行。
- 上游接口只返回最近若干条时，老线可能出现「无候选」；这是提示，不是失败。
