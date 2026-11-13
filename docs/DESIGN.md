# phprtv3 框架

这是设计文档的主入口。它从框架角度说明 phprtv3 由哪些层组成、每层只回答什么问题、
彼此如何约束；recipe 字段、命令语法、输出格式分别写在各子文档里，见第 15 节。

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

## 2. 目标与非目标

**目标：把上面四种信息拆开，每种只有一个来源。**

| 目标 | 判据 |
| --- | --- |
| 新增 PHP 系列不复制依赖 recipe | 加一个系列只新增该系列自己的声明 |
| 依赖版本可选、可换线、可升补丁 | 同一份 recipe 能服务多条 openssl 线 |
| 兼容关系可声明、可校验 | 选错线在求解阶段失败，并给出候选 |
| 一份 recipe 服务多个 PHP 系列 | ext/dep 声明与 PHP 系列数量无关 |

**非目标**：lock 文件、多 PHP 并行构建、LTO。

## 3. 四层数据

```text
catalog.toml             组件 + 线  →  当前最新补丁 / url / sha256
bundles/php<系列>.toml    某个 PHP 系列的默认版本线 + 默认启用扩展
recipes/**.star          每条线支持哪些 PHP / 依赖、怎么构建，以及 PHP 版本事实
CLI + state              本次覆盖哪条线 / 哪些扩展，以及要保留哪些显式选择
```

| 层 | 只回答 | 不回答 | 变更时机 |
| --- | --- | --- | --- |
| catalog | 线 3.5 现在是 3.5 几、从哪下载 | 能不能配 PHP 8.5 | 有补丁发布 |
| bundle | php8.5 默认用 openssl 哪条线、默认启用哪些扩展 | 3.5 具体是 3.5 几、扩展怎么编 | 默认策略调整 |
| recipe | 3.5 这条线支持哪些 PHP、需要哪些库、加什么参数；php recipe 还记录该版本的内置扩展事实 | 3.5 现在是几、默认启用谁 | 兼容性、构建方式或 PHP 版本事实变化 |
| CLI / state | 这次强制用哪条线、上次显式选了哪条 | 线里有哪个补丁 | 每次命令 |

数据流：

```text
        catalog + bundle
               │
               ▼
       ① 选线（bind）──────────────► 冲突：失败或提示
               │  ctx：线 + 补丁 + url + sha256
               ▼
       ② 构造目标 target(ctx)
               │
               ▼
       ③ 依赖闭包 + 拓扑序
               │
               ▼
       ④ 计划输出（文本 / JSON）
               │
               ▼
       ⑤ 下载源码（install / update 默认；--downloads 只下载）
               │
               ▼
       ⑥ 解压源码（构建顺序；--downloads 跳过）
               │
               ▼
       ⑦ 编译安装（按节点签名跳过；失败不写状态）
               │
               ▼
       ⑧ 状态写入（install / update 默认，ext add|remove）
```

补丁号只出现在 catalog。recipe、bundle、CLI 都只谈「线」。

## 4. 三个核心概念

### 4.1 线（line）与补丁（patch）

- **线**是兼容性单位：`openssl 3.5`、`swoole 6.2`、`php 8.5`。
- **补丁**是下载单位：`3.5.9`、`6.2.5`、`8.5.12`。

声明层只允许写线，`version:` 写 `6.2.5` 会被拒绝；补丁由 catalog 决定。因此：

- 安全更新只改 catalog，recipe/bundle 自动跟随；
- 兼容性判断永远发生在线上，不会因为补丁号变化而失效。

### 4.2 兼容性归扩展，不归库

这是 v3 与 v2 最大的结构差异。

| 组件 | 是否声明 PHP 关系 | 原因 |
| --- | --- | --- |
| `ext/swoole.star` | 是，`LINES[线].php` | 扩展自己按 PHP 版本发线 |
| `ext/xdebug.star` | 是，`LINES[线].php` | 同上 |
| `ext/openssl.star` | 是，`PHP_LINES[php] → 允许的库线` | 扩展没有自己的版本线，但“PHP 8.5 的 openssl 扩展能不能配 OpenSSL 4.0”是扩展的事实 |
| `dep/openssl.star` | 否 | OpenSSL 库不认识 PHP |

由此推出：

- dep recipe 声明 `LINES` 或 `PHP_LINES` 在加载阶段直接报错；
- 多线 dep（openssl 有 4 条线）不要求兼容表，只按传入的线选择构建参数；
- 只装 PHP、不装消费扩展时，`--dep openssl=3.5` 不会因为 PHP 版本被拒：库本身不关心 PHP。

`PHP_LINES` 的语义是**允许集合**，不是默认值。它回答“哪些线可以”，不回答“默认用哪条”，
默认由 bundle 决定。

### 4.3 绑定（binding）

一次求解的输出是一张表：

```text
openssl → 线 3.5，补丁 3.5.9，url，sha256
swoole  → 线 6.2，补丁 6.2.5，url，sha256
php     → 线 8.5，补丁 8.5.12，url，sha256
```

这张表是 recipe 的唯一输入上下文。recipe 不自己找版本，只消费绑定结果；也不参与
“选哪条线”的判断，只按拿到的线生成参数。

## 5. 求解流程

### 5.1 确定 PHP 系列

优先级：`install` 位置参数 / `--php` / 唯一的 state。都没有就报错并给示例。

PHP 系列本身也走 catalog：系列 → 当前补丁。

### 5.2 建立初始选择

对每个组件：

```text
CLI 显式(version: / --dep)  >  state.bindings  >  bundle 默认  >  只有一条线时自动
```

按来源分三类：

- **硬选择**（CLI、state）：冲突必须报错，不静默改。
- **软默认**（bundle）：冲突但只有一个候选时自动切换，并打印提示。
- **单线**：没有选择空间，直接绑定。

### 5.3 校验并求约束交集

扩展的约束有两个来源，进入同一个交集：

1. 扩展自己版本线里的依赖约束：`LINES["4.8"]["openssl"] = ["1.1.1"]`
2. builtin 扩展的 PHP 约束：`PHP_LINES["8.5"] = ["1.1.1","3.0","3.5","4.0"]`

```text
allowed = catalog 里该库的所有线
        ∩ 每个已选扩展允许的线
```

处理规则：

| 当前选择 | 结果 |
| --- | --- |
| 在交集里 | 保留 |
| 硬选择不在交集 | 报错，列出可用线 |
| bundle 默认不在交集，交集只有 1 条 | 自动切换 + 提示 |
| bundle 默认不在交集，交集多条或为空 | 报错，要求 `--dep` 指定 |
| 没有初始选择，交集只有 1 条 | 自动绑定 + 提示 |
| 没有初始选择，交集多条 | 报错，要求 `--dep` 指定 |

框架不会替用户在多条合法线里挑一条。

### 5.4 注入上下文并构造目标

选线完成后，才把绑定表交给 recipe。recipe 的职责压缩成一句话：
**给定线，产出构建参数。**

### 5.5 依赖闭包与拓扑

- `needs` 只指向 dep，`exts` 只指向 ext，命名空间不混用。
- static 扩展是 PHP 的子节点；shared 扩展排在 PHP 之后单独展开，避免成环。
- DFS 后序即构建顺序；成环时报出完整路径。
- 无 state 时方案 = bundle `default_exts` - `--disable-default-ext` + `--ext`；
  有 state 时以 `state.ext` 为准，再应用禁用和 `--ext`。
- `[default_exts]` 是 TOML 表（`link` / `features` / `deps`）；有依赖的内置扩展必须一起写，不做依赖展开。

### 5.6 下载源码

`install` / `update` 在计划输出之后、解压之前执行下载；`--downloads` 在这里结束：

```text
计划里的节点 → 源码清单 → 候选 URL → 缓存 / 续传 / 校验 → .phprt/downloads
```

- 清单直接来自求解结果：PHP 本体、dep、PECL 扩展；builtin 扩展的源码在 PHP tarball 里，跳过。
- 缓存命中就跳过；下载失败直接退出，不写状态，也不会进入解压。
- `--downloads` 只下载，不解压、不写状态。
- 缓存按 PHP 系列共享，删除 `.phprt` 即清空。
- 细节见 [DOWNLOAD.md](DOWNLOAD.md)。

### 5.7 解压源码

下载完成后按拓扑序逐个解压，产物就是后续构建的源码根：

```text
源码清单 → build/<组件>/<补丁> → phpize / ./configure
```

- 目标目录已有内容时跳过；失败只删临时目录，不留下半份源码。
- 解压失败直接退出，不写状态；已完成的目录保留。
- 支持 `.tar.gz` / `.tgz` / `.tar.xz` / `.txz` / `.tar.bz2` / `.zip`。
- PECL 包的 `package.xml` + 单个源码目录会提平成源码根。
- 细节见 [EXTRACT.md](EXTRACT.md)。

### 5.8 编译与安装

解压完成后按拓扑序串行构建：

```text
dep → 静态扩展（只贡献 PHP 参数） → php → 共享扩展 → 运行时整理
```

- 每个节点有 configure/make 命令、环境变量、产物模式和签名；签名一致且产物齐全时跳过。
- 共享扩展用树内 `phpize` / `php-config`；builtin 扩展先拷出 PHP 源码里的目录。
- PHP configure 里每个内置扩展只出现正向参数或 `builtin_disable` 的关闭参数其中一组。
- 依赖库收进 `{tree}/lib/phprt`，用 `$ORIGIN` rpath；共享扩展写 `etc/conf.d` ini。
- 最后校验 `php -v` / `php -m`，生成 `myphp` 和 `.phprt/current`。
- 失败打印日志末尾，不写状态。
- 细节见 [BUILD.md](BUILD.md)。

## 6. 两阶段加载

1. **元数据阶段**：只读顶层声明（`TARGET`、`LINES`、`PHP_LINES`、php 的 `series`），
   不调用构建函数。
2. **目标阶段**：选线完成后注入上下文，调用 `target(ctx)` 得到具体目标。

收益是边界清晰：

- 选线不依赖构建函数，构建参数写错不会影响“能不能配”的判断，反之亦然；
- recipe 可以按线返回完全不同的依赖和参数（swoole 4.8 与 6.2 的 needs 不同），
  兼容性仍由声明层统一表达；
- 非法组合在构造目标前就被拒绝，不会输出了半份计划才失败。

代价是同一个 recipe 被求值两次。文件很小，这个代价可以接受。

## 7. 状态：记录方案，不锁版本

状态按 PHP 系列存放：`.phprt/php-<系列>/state.json`。补丁升级不丢方案，安装树
仍然按补丁版本命名。首次写 state 时把当前系列的 `default_exts` materialize 进去；
之后 bundle 默认集变化不再影响已有项目。

| 内容 | 作用 |
| --- | --- |
| 构建方案（扩展、链接方式、依赖集成、feature、installed） | 下次 install/update 装什么 |
| 显式线 `bindings` | 用户用 `version:` / `--dep` 选过的线，下次继续生效 |
| 每条线实际解析到的补丁 | 可观测，不参与求解 |

bundle 默认线不写入 `bindings`：换默认策略后，没有显式选择的组件自然跟随新默认；
显式选过的保持不动。

`update` 只在线内换补丁，不换线；catalog 删掉该线时明确报错，不静默迁移。这是
“不引入 lock”的边界：可复现性靠线，不靠固定补丁号。

## 8. 不变量

框架层面的硬约束，改动不能破坏：

1. 补丁号只出现在 catalog。
2. 版本线的声明只写线，不写补丁号；bundle 的 `default_exts` 只写扩展名/spec。
3. dep 不声明 PHP 关系。
4. 每个组件的线集合在 catalog 和声明之间必须一致。
5. `PHP_LINES` 必须覆盖所有 PHP 系列，引用的线必须存在。
6. 硬选择冲突必须报错；软默认只在唯一候选时自动切换。
7. 默认集只在首次写 state 时 materialize；已有 state 以 state.ext 为准。
   版本线默认仍不落盘。
8. feature 默认只增不减，只有显式移除扩展才允许收窄。
9. 只读命令不写状态。
10. 不实现没有明确要求的用法。

## 9. 演进成本

| 变更 | 需要改的地方 | 不需要改的地方 |
| --- | --- | --- |
| 升 openssl 3.5 补丁 | catalog 一行 | recipe、bundle、state |
| 新增 openssl 4.1 线 | catalog 一条 + 消费扩展的允许集合 | dep 构建参数（同 major 可复用） |
| 新增 PHP 8.7 | catalog 一条 + bundle 一份 + php 声明一份 + 各扩展补兼容 | dep recipe |
| 新增扩展 | 一份扩展声明（含 `LINES` 或 `PHP_LINES`） | 求解器 |
| 换默认线 | bundle 一行 | recipe、catalog |
| 换某条线构建参数 | 对应 recipe 的该线分支 | catalog、bundle |

对比 v2：新增 PHP 版本要复制整套声明，升补丁要改 recipe 并重新验证构建参数。
v3 把这两件事分别压到一条 catalog 记录和一行 bundle 记录。

## 10. 失败模式：宁可报错，不猜

框架对以下情况显式失败，并给出候选：

- PHP 版本未指定；
- 线不存在 / 组件没有版本表 / 把补丁当线写；
- 线不支持当前 PHP；
- 显式选择与扩展约束冲突；
- 多条合法线但没有默认；
- 依赖关系指向不存在的目标、成环；
- dep 声明了 PHP 关系；
- 状态里有 recipe 已不存在的扩展或 feature。

允许自动处理的只有一种：**软默认在唯一候选时切换，并打印提示。**
其余情况不替用户做决定。

## 11. 仓库中的落点

数据层：

| 层 | 位置 |
| --- | --- |
| catalog | `recipes/catalog.toml` |
| 下载配置 | `recipes/phprt.toml` |
| bundle | `recipes/bundles/php<系列>.toml` |
| 目标声明 | `recipes/php<系列>.star`、`recipes/dep/*.star`、`recipes/ext/*.star` |
| 构造函数契约 | `recipes/defs.star` |
| 本次覆盖 | 命令行 |
| 方案与显式线 | `.phprt/php-<系列>/state.json` |

驱动侧按角色拆成模块，不出现具体包名：

| 角色 | 模块 | 职责 |
| --- | --- | --- |
| 入口 | `src/main.rs` | 命令 → PHP 系列 → 串接其余角色、退出码 |
| 版本库 | `src/catalog.rs` | 线/补丁查询、PHP 系列解析 |
| 默认线 / 默认集 | `src/bundle.rs` | 读取扩展默认线、`[deps]` 库默认线和 `default_exts` |
| 选线 | `src/bind.rs` | 优先级、兼容校验、约束交集、生成上下文 |
| 声明 | `src/recipe.rs` | 两阶段加载、契约校验、内置扩展事实、`--ext` 解析 |
| 依赖 | `src/plan.rs` | 依赖边、拓扑序、成环检测 |
| 输出 | `src/render.rs` | 占位符展开、文本与 JSON |
| 选项盘点 | `src/audit.rs` | 读上游 config.m4 / configure --help，检查 recipe 的处置 |
| 状态 | `src/state.rs` | 方案、显式线、feature 并集、写回 |
| 路径 | `src/layout.rs` | 压缩包名、源码 stem、`build/<组件>/<补丁>` |
| 清单 | `src/sources.rs` | 求解结果 → 下载/解压共用的 `Source` 列表 |
| 构建计划 | `src/build.rs` | 求解结果 → 绝对路径的构建步骤、环境、rpath、入口 |
| 执行器 | `crates/build` | 按步骤执行、日志、签名跳过、产物检查 |
| 下载 | `crates/download` | 候选 URL、缓存与续传、分片传输 |
| 解压 | `crates/extract` | 格式分派、tar/zip 解包、跳过与失败清理 |
| 进度 | `crates/ui` | 进度框、逐行日志、标签与大小格式 |
| 清单类型 | `crates/source` | `Source` 结构，供下载和解压共用 |

目录：

```text
phprtv3/
├── Cargo.toml                  # workspace + phprt 根包
├── src/
│   ├── main.rs
│   ├── catalog.rs
│   ├── bundle.rs
│   ├── bind.rs
│   ├── recipe.rs
│   ├── plan.rs
│   ├── render.rs
│   ├── state.rs
│   ├── layout.rs
│   ├── sources.rs
│   └── build.rs
├── crates/
│   ├── source/                 # 下载/解压共用的 Source 类型
│   ├── ui/                     # 进度框与逐行日志
│   ├── download/               # candidate / store / transfer
│   ├── extract/                # archive / 解压流程
│   └── build/                  # 构建步骤执行、日志、签名、产物
├── templates/
│   └── myphp.tmpl              # 项目入口模板
├── vendor/perl/                # 构建期补系统 perl 的纯 Perl 模块
├── recipes/
│   ├── catalog.toml
│   ├── bundles/php{7.4,8.0,8.2,8.3,8.4,8.5,8.6}.toml
│   ├── defs.star
│   ├── php{7.4,8.0,8.2,8.3,8.4,8.5,8.6}.star
│   ├── dep/{openssl,libcurl,nghttp2,autoconf}.star
│   └── ext/{openssl,curl,swoole,xdebug}.star
└── docs/
    ├── DESIGN.md
    ├── CONTRACT.md
    ├── CLI.md
    ├── SOLVER.md
    ├── OUTPUT.md
    ├── STATE.md
    ├── DOWNLOAD.md
    ├── EXTRACT.md
    └── BUILD.md
```

## 12. 不在范围内

- 在线更新 catalog；
- 精确补丁锁定（lock）；
- 多 PHP 同时构建、LTO、`--static-all`；
- 产物签名与分发。

这些都能在现有四层之上追加，不需要改声明结构：

- 在线 catalog → 换掉 catalog 的来源，结构不变；
- lock → 在 state 之上增加补丁固定层，不影响“线”的语义。

## 13. 走查：`install 8.5 --ext openssl` 为什么是 3.5.9

| 步骤 | 来源 | 结果 |
| --- | --- | --- |
| 1 | catalog `[php]` | 系列 8.5 → 补丁 8.5.12 |
| 2 | bundle `php8.5.toml` | openssl 默认线 3.5 |
| 3 | `ext/openssl.star` 的 `PHP_LINES["8.5"]` | 允许 1.1.1 / 3.0 / 3.5 / 4.0 |
| 4 | 交集 | 3.5 在允许集合内，保留 3.5 |
| 5 | catalog `[openssl]` | 3.5 → 3.5.9 + url + sha256 |
| 6 | `target(ctx)` | 按 line=3.5 选 `./Configure` 与 artifacts，version 用 3.5.9 |

4.0 是“允许”，不是“默认”。要 4.0 必须显式指定：

```bash
phprt install 8.5 --ext openssl --dep openssl=4.0
```

只装 swoole、不装 ext:openssl 时，openssl 线由 swoole 自己的
`LINES["6.2"]["openssl"]` 约束，不经过 `PHP_LINES`。

## 14. v2 与 v3 对照

| 维度 | v2 | v3 |
| --- | --- | --- |
| 组织单位 | PHP 版本 | PHP 系列 + 组件线 |
| 依赖版本 | 写死在 recipe | catalog 按线提供 |
| 新增 PHP | 复制整套 recipe | 新增 php 声明 + bundle + catalog 记录 |
| 换依赖版本 | 改 recipe 或复制 | 改 bundle / CLI / catalog |
| 升级依赖补丁 | 改 recipe | 改 catalog |
| 兼容关系 | 隐含在抄写的版本里 | `LINES` / `PHP_LINES` 显式声明并校验 |
| 多 PHP 共存 | 结构上不支持 | 按系列独立存放方案 |
| 冲突处理 | 无 | 硬选择报错，软默认唯一候选才切换 |

## 15. 子文档

| 文档 | 内容 |
| --- | --- |
| [CONTRACT.md](CONTRACT.md) | recipe 字段、ctx、LINES/PHP_LINES、catalog、bundle、校验规则 |
| [ADD-RECIPE.md](ADD-RECIPE.md) | 新增/更新版本线与补丁、下载地址跳转判定、catalog 注释规则 |
| [CLI.md](CLI.md) | 子命令、`--php`、`--ext`、`--dep`、错误与退出码 |
| [SOLVER.md](SOLVER.md) | 选线、依赖线交集、依赖边、拓扑顺序、feature 传播 |
| [OUTPUT.md](OUTPUT.md) | 文本/JSON 输出、路径规则、字段含义 |
| [STATE.md](STATE.md) | state.json v2、bindings、line/version、写入时机 |
| [DOWNLOAD.md](DOWNLOAD.md) | `--downloads`、缓存布局、镜像、分片与续传、进度输出 |
| [EXTRACT.md](EXTRACT.md) | 解压顺序、目录布局、格式、原子解压、安全与失败 |
| [BUILD.md](BUILD.md) | 构建顺序、环境、签名跳过、日志、rpath、ini、入口、验收 |
| [FEATURES.md](FEATURES.md) | 上游开关盘点、feature 取舍、默认值判据、`audit` |

## 16. 验收

```bash
rm -rf .phprt build          # 从干净状态开始
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all --check

cargo run -- install 8.5                                  # 默认 openssl + curl(ssl) + libxml
cargo run -- install 8.5 --disable-default-ext openssl --disable-default-ext curl --disable-default-ext libxml  # 只装 PHP
cargo run -- install 8.5 --ext swoole --ext xdebug        # 加上两个 PECL 扩展
cargo run -- install 8.5 --ext 'curl=link:shared,dep:libcurl[ssl,nghttp2]'
cargo run -- install 8.5 --ext openssl --dep openssl=4.0  # openssl 4.0.0
cargo run -- install 7.4 --ext openssl --dep openssl=3.5  # 必须失败
cargo run -- install 8.2 --ext swoole=version:4.8         # 提示自动改用 1.1.1
cargo run -- install                                      # 必须提示指定 PHP
cargo run -- --php 8.5 ext list --installed
cargo run -- --php 8.5 update

cargo run -- install 8.5 --downloads                          # 只下载，不写 state
cargo run -- install 8.5 --ext swoole --downloads --offline   # 缓存命中路径
cargo run -- install 8.5 --ext swoole --offline               # 下载+解压+编译+写 state
```

默认的 `install` / `update` 会写状态（构建方案和显式线），所以验收从
`rm -rf .phprt build` 开始，避免上一条的扩展方案、`bindings` 和解压目录影响下一条。
`--downloads` 只下载，不写 state；需要网络时直连失败会自动换镜像。

测试覆盖：所有 PHP 系列的默认线、bundle `default_exts`、两次 materialize 语义、
内置扩展事实与 `builtin_disable`、`--disable-default-ext` 冲突/范围、软默认自动切换、
硬选择冲突、版本线不支持 PHP、builtin 扩展的 `PHP_LINES`、dep 不允许声明 PHP 关系、
`--ext` 的 version 预解析、
catalog PHP 版本解析、下载的 Range 分片/续传/镜像顺序/重试/校验，以及原有的
依赖闭包/拓扑/契约测试，解压的四种格式、顶层提平、PECL 包提平、跳过、越界拒绝和
失败清理，以及构建步骤的 glob、签名和命令拼装。
