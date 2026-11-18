# phprtv3 框架

这是设计文档的主入口。它从框架角度说明 phprtv3 由哪些层组成、每层只回答什么问题、
彼此如何约束；recipe 字段、命令语法、输出格式分别写在各子文档里，见第 14 节。

## 1. 起点：v2 是单版本式方案

v2 的组织方式是「一个 PHP 版本 = 一整套 recipe」：

```text
php8.5.star
├── PHP 版本 + 下载地址
├── 依赖库版本 + 下载地址
├── 扩展版本 + 下载地址
└── 全部编译参数
```

只支持一个 PHP 版本时够用，扩大支持范围后暴露四个问题：

| 问题 | 表现 |
| --- | --- |
| 重复开发 | 支持 8.2 就复制一份 8.5 的声明，改版本号、改包名；7.4 的 openssl 参数不同，再复制一份 |
| 版本焊死 | 依赖版本写在 recipe 里，一份 recipe 只能表达「openssl 3.5.9」。想换 4.0 或升 3.5.10，只能改 recipe 或再加一份 |
| 兼容关系无处安放 | “PHP 8.5 能用哪些 openssl 线”只存在于作者当时抄了哪个版本，机器无法校验，也无法解释为什么是这个版本 |
| 补丁更新污染声明 | 安全更新只改补丁号，但它和构建参数混在同一份文件里，每次更新都要重新 review 整个 recipe |

根因是把四种寿命不同的信息写在了一起：

- 兼容关系（按版本发版节奏变）
- 默认策略（偶尔变）
- 具体补丁和下载地址（每次上游发版都变）
- 本次构建的临时选择（每次命令都变）

v3 把它们拆开，同时把“只服务 PHP”推广成“产品 / 组件 / 依赖库”三层。

## 2. 目标与非目标

**目标：把四种信息拆开，每种只有一个来源；同一套模型能构建 PHP 之外的产品。**

| 目标 | 判据 |
| --- | --- |
| 新增产品系列不复制依赖 recipe | 加一个系列只新增该系列自己的声明 |
| 依赖线由约束求交决定 | 兼容关系写在 recipe，默认线写在 bundle，当前选择来自 state / CLI |
| 补丁号只来自 catalog | recipe、bundle 不写补丁号；state 记录装过的版本，`--lock` 用它精确复用（源码仍取 catalog） |
| 产品之间不互相绑定 | 一个可执行文件 `phprt`，一份下载缓存，安装树按产品隔离 |
| 组件挂载方式由产品定义 | PHP 的 static/shared 和 nginx 的 builtin/dynamic 走同一套模型 |

**非目标：**

- 不做跨产品共享安装树；每个产品一份 `.phprt/<产品>-<版本>/`。
- 不做默认版本、当前产品、`phprt.toml` 项目清单。
- 不做一条命令安装多个产品。
- 不生成独立 lock 文件；补丁复用与 `--lock` 见 [LOCK.md](LOCK.md)。
- 不做交叉编译、LTO、`--static-all`。

## 3. 四层数据

| 层 | 文件 | 只回答 |
| --- | --- | --- |
| catalog | `recipes/catalog.toml` | 这条线当前是哪个补丁、从哪下载 |
| recipe | `recipes/products/**`、`components/**`、`dep/**` | 怎么编译、允许哪些线、需要哪些依赖 |
| bundle | `recipes/bundles/<产品>/<系列>.toml` | 默认用哪条线、默认装哪些组件 |
| state | `.phprt/<产品>-<系列>/state.json`（工具目录在项目根下，默认 `.phprt`，可配置） | 本机这次实际选了什么、装到哪一步 |

优先级从低到高：catalog < recipe < bundle < state < CLI。

## 4. 三个核心概念

### 4.1 线（line）与补丁（patch）

```text
线   8.5 / 1.29 / 6.2 / 3.5
补丁 8.5.12 / 1.29.8 / 6.2.5 / 3.5.9
```

- 用户和 recipe 只写线；补丁只写在 catalog。
- 换补丁不改 recipe、bundle；state 只跟着记录实际装过的版本。
- 一个产品有多个系列时，每个系列一份产品 recipe、一份 state、一棵安装树。

### 4.2 兼容性归产品与组件，不归库

- dep（OpenSSL、libcurl、PCRE2）只描述库本身，不声明属于哪个产品。
- 产品 recipe 的 `LINES[系列]` 约束自己的依赖线。
- 组件 recipe 的 `LINES[线].product` 约束支持哪些产品系列，其他 key 约束依赖线。
- bundled 组件用 `PRODUCT_LINES` / `PRODUCT_DEP_LINES` 约束同名 / 不同名依赖。
- 默认线只在 bundle；显式线来自 state / CLI。

### 4.3 绑定（binding）

一次安装对每个组件、依赖库选一条线，求解结果是 `Bindings`：

```text
swoole = 6.2
openssl = 3.5
libcurl = 8.23
```

绑定不写进 recipe；安装成功后把用户显式指定的部分写进 `state.bindings`。

## 5. 求解流程

### 5.1 确定产品和系列

```bash
phprt install php@8.5
```

产品、系列都来自命令行的唯一目标。没有默认产品，也没有“当前产品”。

### 5.2 建立初始选择

```text
CLI --component version: / --dep
  > state.bindings
  > bundle 默认线（组件写顶层，依赖库写 [deps]）
  > 组件只有一条线时直接用
```

### 5.3 校验并求约束交集

- 组件的 `LINES[线].product` 必须包含当前系列；不满足时显式选择报错，bundle 默认在
  只有一个候选时自动切换。
- 产品 `LINES[系列]`、组件的 `LINES[线]`、`PRODUCT_LINES`、`PRODUCT_DEP_LINES`
  共同约束每条依赖线，取交集。
- 交集为空、或显式 pin 不在交集里，报错并列出候选。

### 5.4 注入上下文并构造目标

Rust 把产品名、系列、补丁、绑定和依赖记录注入 `ctx`，调用 recipe 的 `target(ctx)`；
产品 recipe 的版本和下载地址直接由 catalog 注入。

### 5.5 依赖闭包与拓扑

```text
dep → fold 组件 → product → standalone 组件
```

- fold 组件是产品的子节点，只贡献源码路径和 configure 参数。
- standalone 组件在产品之后展开，依赖产品的可执行文件（phpize / php-config 等）。
- 成环检测、DFS 后序、工具依赖只在 standalone 时进 PATH，规则不变。

### 5.6 下载源码

按 catalog 的 URL 下载到共享缓存 `.phprt/downloads/`；缓存文件名冲突时报错。
`--downloads` 在这里结束。

### 5.7 解压源码

按拓扑序解压到 `build/<名字>/<源码目录名>`；顶层只有一个目录时提升一层。
bundled 组件不单独下载，源码在产品 tarball 里。

### 5.8 编译与安装

按拓扑序执行 recipe / profile 生成的步骤；每个节点一份日志、一份签名。
安装树是 `.phprt/<产品>-<版本>/`；依赖库装在自己的 `deps/<名字>-<版本>/`。
全局模式（`--global --prefix <目录>`）下安装树就是 prefix，状态和完成标记也放在里面。

## 6. 两阶段加载

1. **元数据阶段**：只读 recipe 顶层，拿到产品的系列 / bundled 事实、
   组件的 `LINES` / `PRODUCT_LINES` / `PRODUCT_DEP_LINES`，用于选线和校验。
2. **目标阶段**：带着 ctx 求值 `TARGET`，得到具体参数，再构造 `Registry`。

元数据阶段不做构建，不读 state；目标阶段不再改版本线。

## 7. 状态：记录方案与实际版本

```json
{
  "version": 3,
  "product": "php",
  "series": "8.5",
  "components": { "swoole": { "line": "6.2", "mode": "shared", "installed": true } },
  "deps": { "openssl": { "line": "3.5", "version": "3.5.9" } }
}
```

- `state.components` 是权威方案；`plan` / `install` 都从它出发。
- 版本线记录在 state，补丁号记录实际装过的版本。
- 已装版本是否复用、`--lock` 与 catalog 旧补丁保留见 [LOCK.md](LOCK.md)；
  state 是版本来源，不新增 lock 文件。
- 旧 schema（version 2）不读取，不自动迁移，报错要求重新安装。
- 多产品各自一份 state；`list` 扫描 `.phprt/*/state.json` 汇总。

## 8. 不变量

1. 命令只接受一个 `产品@系列`；多产品 = 多条命令。
2. 组件 spec 一次只写一个组件；多个组件分多条命令。
3. 补丁号只出现在 catalog。
4. 默认线只出现在 bundle；显式线来自 state / CLI。
5. 同一个产品的多个系列并存，互不影响；不同产品不共享安装树。
6. 失败不写 state；写入是最后一步。
7. 产品 profile 只声明步骤模板、路径和验证命令，不写条件分支。

## 9. 演进成本

| 变更 | 要改的地方 |
| --- | --- |
| 升补丁 | catalog 一行 |
| 换默认线 | bundle 一行 |
| 加组件 | `components/<产品>/<名字>.star` + 需要时 catalog |
| 加产品系列 | 产品 recipe + bundle + catalog |
| 加产品 | 产品 recipe + bundle + catalog + 需要时新 dep |
| 加依赖线 | catalog + 消费方的约束 |

## 10. 失败模式：宁可报错，不猜

- 未知产品 / 系列 / 组件 / 依赖：直接报错并列出候选。
- 裸名有多个候选：要求写 `kind:name`，产品节点写 `product:<名字>@<版本>`。
- 约束冲突：报冲突的线、可用线，不自动降级。
- 组件契约不支持某个 mode：报支持列表。
- state 与 recipe 不一致：报错，要求改 state 或重新安装。
- 签名变化、源码缺失、产物缺失：重编；命令失败时打印日志末尾，不写 state。

## 11. 仓库中的落点

```text
src/
├── main.rs       CLI、命令分派、状态读写
├── catalog.rs    catalog.toml
├── recipe.rs     两阶段加载、契约校验、Registry、Selection
├── bundle.rs     bundles/<产品>/<系列>.toml
├── bind.rs       版本线求解
├── plan.rs       依赖边与拓扑
├── render.rs     plan 的文本 / JSON、占位符展开
├── build.rs      构建步骤、环境、运行时整理、验证、入口
├── marker.rs     完成标记、方案指纹、install 判定
├── manage.rs     add / remove 的现场检查、确认交互、ini 备份、产物删除
├── audit.rs      上游选项盘点
├── state.rs      state.json
├── layout.rs     源码 / 安装路径
├── sources.rs    下载清单
└── config.rs     下载 / rpath 配置

crates/
├── download/     下载、镜像、分片续传、进度
├── extract/      解压、原子目录、路径安全
├── build/        命令执行、签名、日志、产物检查
├── source/       下载清单类型
├── ui/           进度显示
└── rpath/        rpath 设置（oxipatch 库 / patchelf 命令双后端，见 RPATH.md）

recipes/
├── products/<产品>/<系列>.star
├── components/<产品>/<名字>.star
├── dep/<名字>.star
├── bundles/<产品>/<系列>.toml
├── catalog.toml
└── defs.star
```

`crates/download`、`crates/extract`、`crates/build` 只接收“工作目录 + 命令 + 环境 + 产物”，
不关心产品是 PHP 还是 nginx。

## 12. 不在范围内

- 项目清单、当前产品、默认版本；
- 一条命令构建多个产品；
- 跨产品共享安装树；
- 独立 lock 文件（精确版本复用见 [LOCK.md](LOCK.md)）；
- 交叉编译、LTO、`--static-all`。

## 13. 走查：`install php@8.5 --component openssl` 为什么是 3.5.9

```text
命令行：产品 php，系列 8.5，组件 openssl（无 version:）
bundle：php/8.5.toml 的 [deps].openssl = 3.5
state：没有显式 binding
catalog：openssl 3.5 → 3.5.9
```

组件 `openssl` 的 `PRODUCT_LINES["8.5"]` 包含 3.5，所以 3.5 合法；补丁 3.5.9 来自 catalog。

如果用户写：

```bash
phprt install php@8.5 --component openssl --dep openssl=4.0
```

显式 pin 4.0 也在 `PRODUCT_LINES["8.5"]` 里，于是选 4.0.0；如果写 7.4 系列，4.0 不在
允许线里，直接报冲突。

## 14. 子文档

| 文档 | 内容 |
| --- | --- |
| [CLI.md](CLI.md) | 子命令、`产品@系列`、`--component`、`--dep`、错误与退出码 |
| [CONTRACT.md](CONTRACT.md) | recipe 字段、ctx、`LINES`、`PRODUCT_LINES`、profile、bundle |
| [PRODUCT.md](PRODUCT.md) | 产品共享 profile、工具最低版本、安装后动作、入口模板 |
| [SOLVER.md](SOLVER.md) | 选线、依赖线交集、依赖边、feature 并集 |
| [OUTPUT.md](OUTPUT.md) | 文本 / JSON 输出、路径、字段 |
| [STATE.md](STATE.md) | state.json、bindings、line/version |
| [LOCK.md](LOCK.md) | 解析顺序、`--lock`、`update`、`scheme --lock`、catalog 旧补丁保留 |
| [DOWNLOAD.md](DOWNLOAD.md) | 下载、缓存、镜像、分片续传、进度 UI |
| [EXTRACT.md](EXTRACT.md) | 解压顺序、目录布局、格式、原子解压、安全与失败 |
| [BUILD.md](BUILD.md) | 构建顺序、环境、签名跳过、日志、rpath、验证、入口 |
| [RPATH.md](RPATH.md) | rpath 双后端（oxipatch 库 / patchelf 命令）、配置切换、差分验收（待实施方案） |
| [FEATURES.md](FEATURES.md) | 上游开关盘点、feature 取舍、默认值判据、`audit` |
| [ADD-RECIPE.md](ADD-RECIPE.md) | 新增产品 / 组件 / 版本线、catalog 地址判定 |
| [SWOOLE.md](SWOOLE.md) | Swoole 各版本线的开关盘点、处置、默认值与验收 |
| [FLOW.md](FLOW.md) | install / add / remove / scheme 的流程、判定矩阵、交互、`--force`、ini 备份 |
| [GENERIC.md](GENERIC.md) | 通用化设计的完整走查；与本文冲突时以本文和 CONTRACT.md 为准 |

## 15. 验收

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets

cargo run -- install php@8.5
./myphp-8.5 -v
cargo run -- plan php@8.5 --json

cargo run -- install nginx@1.29
./.phprt/nginx-1.29.8/sbin/nginx -V
cargo run -- add nginx@1.29 lua-nginx-module
cargo run -- install nginx@1.29
ls .phprt/nginx-1.29.8/modules/

cargo run -- list
cargo run -- install php@8.5 nginx@1.29   # 必须失败：一次只能一个产品
```
