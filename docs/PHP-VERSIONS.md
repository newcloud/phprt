# phprt 多 PHP 版本配置组织方案（讨论稿）

- 状态：讨论中，未实施
- 日期：2026-11-04
- 适用仓库：本仓库（`phprt`）
- 涉及版本：现有 `8.5.12`、`8.6.0-dev`，计划新增 `7.4.33`、`8.6` 正式版
- 后续：本文的 profile/继承方案与 `docs/PROJECT-LOCAL.md` 是两条路线；当前讨论转向项目本地自包含模型。

## 摘要

现有架构已经支持「每个 PHP 版本一份配置」，也支持不同版本使用不同依赖版本：`dep_dir_of()` 按当前配置里的版本变量计算目录，`.phprt-deps` 按 PHP 版本记录实际链接的依赖。

真正需要补的是三件事：

1. 配置的组织单位。8.5 与 8.6 大部分相同，不应各维护一份完整 `config.conf`；7.4 差异大，应有一个独立、直白的目录。
2. configure 参数与扩展能力随 PHP 版本变化。`builtin/*.sh` 目前把 8.x 的参数拼写写死，7.4 的 `--with-libxml-dir`、`--with-gd` 等无法表达。
3. 依赖版本有硬约束，而且是传递的。7.4 不能使用 OpenSSL 3.x；同一个依赖版本在不同配方下也不能共享同一个安装目录。

本方案采用「系列目录 + 文本 include + 最近同名文件覆盖」的组织方式，不引入范围矩阵解析器。安装根只保存指针和用户覆盖，完整配置由仓库 profile 链解析；需要复现时用 `freeze` 生成快照。

## 1. 现状与问题

### 1.1 已经具备的能力

- `lib/versions/php-<版本>.conf` 保存 PHP 身份：版本、URL、SHA256、标签。
- `install.sh` 把版本文件和仓库 `config.conf` 拼接成 `<安装根>/configs/php-<版本>.conf`。
- `lib/modules/deps/*.sh` 通过版本变量计算安装目录，例如 `OPENSSL_VERSION` → `deps/openssl-<版本>/`。
- 每个 `php-<版本>/` 有独立的 stamp、`etc/php.ini`、扩展目录；`.phprt-deps` 记录该版本链接了哪些依赖目录。
- `--prune` 依据 `.phprt-deps` 保留已被引用的依赖目录。

因此，「7.4 用 OpenSSL 1.1.1w、8.5 用 3.5.9」在数据上已经可以做到，只要两份配置写入不同的 `OPENSSL_VERSION`。缺少的是配置继承、参数适配和配方隔离。

### 1.2 现在不支持的情况

| 场景 | 现状 |
| --- | --- |
| 8.5 与 8.6 只有两三处不同 | 需要复制整份 `config.conf`，两份内容长期漂移 |
| 7.4 的 configure 参数拼写不同 | `lib/modules/builtin/*.sh` 写死 8.x 参数，无法按版本切换 |
| 同一逻辑扩展从内置改为 PECL | 一个 `ENABLE_*` 开关只能认领一个文件，无法按 PHP 系列选择实现 |
| 7.4 需要旧版 OpenSSL | 可以改版本变量，但没有约束校验，配置写错时会在编译中途失败 |
| 同一个依赖版本需要不同编译配方 | `deps/<名字>-<版本>/` 目录名不含配方，stamp 可能复用错误产物 |
| 8.6 新增一个可选扩展 | 需要同时改 `config.conf`、`enabled_module()`、`ALL_MODULES`、`verify.sh` |

### 1.3 代码里的硬编码点

这些位置决定了「加扩展」目前不是「丢一个文件」：

- `build.sh`：`LIB_MODULES`、`EXT_MODULES`、`ALL_MODULES`
- `build.sh`：`enabled_module()` 的开关到模块映射
- `lib/common.sh`：`builtin_order()`、`stamp_file()`、`dep_dir_of()`、`php_sig()`
- `lib/verify.sh`：`check_ext` 的扩展清单
- `install.sh`：`--profile minimal` 对整份配置做 `sed`

本方案不要求第一步就全部动态化，但新增 7.4、8.6 时应优先消除与版本能力相关的硬编码。

## 2. 设计原则

1. 目录即作用域。打开 `profiles/php-7.4/` 就能看出哪些东西是专门给 7.4 的。
2. 逻辑开关与实现分离。`ENABLE_GD` 是逻辑能力，7.4 和 8.5 用哪个参数、哪个 provider 由目录决定。
3. 共享用引用，差异用覆盖。相同内容只写一份；版本差异写在最近的一层。
4. 仓库是配置源，安装根是构建产物。安装根不再保存完整配置副本。
5. 依赖身份包含配方。版本号相同的依赖，如果编译参数不同，必须落到不同目录。
6. 约束要能校验。越界的组合在构建前报错，而不是编译十分钟后失败。
7. 不引入隐藏默认值。解析结果必须能通过 `phprt config tree` 看到来源。

## 3. 目录结构

按系列建目录，版本文件只保存身份和指向哪个系列。某个 patch 版本需要偏离时，再加一层更具体的目录。

```text
lib/
├── versions/
│   ├── php-8.5.12.conf          # 身份 + PHP_PROFILE=php-8.5
│   ├── php-8.6.0.conf           # 身份 + PHP_PROFILE=php-8.6
│   └── php-7.4.33.conf          # 身份 + PHP_PROFILE=php-7.4
├── profiles/
│   ├── shared/
│   │   ├── base.conf            # 所有版本共用的开关、工具链、代理
│   │   └── deps/
│   │       ├── openssl-3.3.conf
│   │       ├── openssl-3.5.conf
│   │       ├── openssl-1.1.1w.conf
│   │       └── ...
│   ├── php-8.5/
│   │   ├── profile.conf
│   │   └── deps/
│   │       ├── openssl.conf
│   │       └── libxml2.conf
│   ├── php-8.6/
│   │   ├── profile.conf
│   │   └── deps/
│   │       └── xdebug.conf
│   ├── php-8.5.12/              # 可选：只有这个 patch 需要偏离时存在
│   │   └── deps/
│   │       └── openssl.conf
│   └── php-7.4/
│       ├── profile.conf
│       ├── deps/
│       │   ├── openssl.conf
│       │   ├── libxml2.conf
│       │   ├── icu.conf
│       │   └── ...
│       ├── builtin/
│       │   └── gd.sh
│       └── patches/
└── modules/                     # 现有基础实现
```

### 3.1 版本文件

```conf
# lib/versions/php-8.5.12.conf
PHP_VERSION=8.5.12
PHP_URL="https://www.php.net/distributions/php-8.5.12.tar.xz"
PHP_SHA256=""
PHP_LABEL="最新稳定"
PHP_PROFILE=php-8.5
```

版本文件不写依赖版本、开关和编译参数。

### 3.2 profile.conf

`profile.conf` 只写这个系列相对父系列的差异。

```conf
# lib/profiles/php-8.6/profile.conf
# phprt: extends=php-8.5

ENABLE_URI=1
```

```conf
# lib/profiles/php-7.4/profile.conf
# phprt: extends=base

PHP_EXTRA_CFLAGS="-fcommon -Wno-error=implicit-function-declaration -Wno-error=incompatible-pointer-types"
DEP_FLAVOR=legacy74
```

`extends` 决定目录叠加链。`php-8.6` 没有写 `extends` 时就只继承 `base`；写了 `extends=php-8.5` 才继承 8.5 的差异。跨系列继承必须显式声明，避免 8.5 的修改悄悄影响 8.6。

### 3.3 依赖片段

每个依赖文件是该组件的完整配方，不只写版本号：

```conf
# lib/profiles/shared/deps/openssl-3.3.conf
OPENSSL_VERSION=3.3.0
OPENSSL_URL="https://www.openssl.org/source/openssl-3.3.0.tar.gz"
OPENSSL_SHA256=""
OPENSSL_BUILD_OPTS="shared no-tests"
DEP_FLAVOR=modern
```

系列目录里的 `deps/openssl.conf` 通过 include 引用共享片段：

```conf
# lib/profiles/php-8.5/deps/openssl.conf
# 共享片段：修改 shared/deps/openssl-3.3.conf 会影响所有引用它的系列
phprt_include ../../shared/deps/openssl-3.3.conf
```

只有 8.5.12 要改用 3.5 时，在更具体的目录放同名文件：

```conf
# lib/profiles/php-8.5.12/deps/openssl.conf
phprt_include ../../shared/deps/openssl-3.5.conf
```

### 3.4 模块与补丁覆盖

- `modules/`：最近的同名文件赢。7.4 需要不同参数时，只覆盖 `builtin/gd.sh`，不复制整套模块。
- `patches/`：沿继承链累加。基础补丁在前，具体版本补丁在后。
- 补丁和覆盖文件的内容必须计入 `php_sig`，否则 stamp 命中后不会重编。

## 4. 解析规则

### 4.1 继承链

以 `8.5.12` 为例：

```text
profiles/php-8.5.12/        # 可选，patch 级覆盖
  → profiles/php-8.5/       # 系列 profile
  → profiles/shared/        # base.conf 与共享依赖片段
```

以 `8.6.0` 为例：

```text
profiles/php-8.6/           # extends=php-8.5
  → profiles/php-8.5/
  → profiles/shared/
```

以 `7.4.33` 为例：

```text
profiles/php-7.4/           # extends=base
  → profiles/shared/
```

### 4.2 各类文件的合并方式

| 文件 | 规则 |
| --- | --- |
| `profile.conf` | 沿 `extends` 链累加，父级先加载，子级后加载并覆盖同名变量 |
| `deps/<组件>.conf` | 最近的同名文件赢，不累加；文件内可用 `phprt_include` 引用共享片段 |
| `modules/...` | 最近的同名文件赢 |
| `patches/` | 沿链累加，顺序为 base → 系列 → 版本 |
| `<安装根>/configs/php-<版本>.local.conf` | 最后加载，覆盖以上所有层 |

### 4.3 include 的语义

`phprt_include <相对路径>` 是 `lib/common.sh` 提供的 shell 函数，按当前被加载文件的位置解析路径，再把目标文件 source 进来。它有两个用途：

1. 引用共享依赖片段，例如 `phprt_include ../../shared/deps/openssl-3.3.conf`。
2. 在 `profile.conf` 中引入公共变量文件。

系列继承使用 `extends`，不使用 include。这样目录叠加查找不依赖解析文件内容，行为更确定。

如果后续希望全部统一成 include，也可以让加载器识别被 include 的 `profile.conf` 并自动建立继承链，但可读性和可调试性不如显式 `extends`。

### 4.4 变量优先级

从低到高：

1. `profiles/shared/base.conf`
2. 父系列 `profile.conf`
3. 当前系列 `profile.conf`
4. 当前系列 `deps/<组件>.conf`
5. patch 级 profile 与 deps
6. `<安装根>/configs/php-<版本>.local.conf`
7. `--profile minimal` 等命令行临时层

第 7 层只影响本次构建，不写回仓库。

## 5. 三个场景

### 5.1 8.6 与 8.5 基本相同

`profiles/php-8.6/` 只放差异：

```conf
# profile.conf
# phprt: extends=php-8.5

ENABLE_URI=1
```

```conf
# deps/xdebug.conf
phprt_include ../../shared/deps/xdebug-3.6.conf
```

没有差异时，目录可以只有 `profile.conf` 中的一行 `extends`。8.5 的依赖链接被 8.6 继承；如果 8.6 不想跟随某次 8.5 的修改，就在 8.6 目录放自己的同名文件。

### 5.2 7.4 使用旧版依赖

`profiles/php-7.4/deps/` 里的每个文件都指向 7.4 可用的片段：

```conf
# deps/openssl.conf
phprt_include ../../shared/deps/openssl-1.1.1w.conf

# deps/libxml2.conf
phprt_include ../../shared/deps/libxml2-2.9.14.conf
```

7.4 的参数拼写差异只在 `profiles/php-7.4/builtin/gd.sh` 等少数文件里处理，不散落到通用模块。

依赖候选如下，均需实测确认：

| 组件 | 8.x 候选 | 7.4 候选 | 原因 |
| --- | --- | --- | --- |
| OpenSSL | 3.5.9 | 1.1.1w | 7.4 的 `ext/openssl` 不支持 3.x |
| libxml2 | 2.15.4 | 2.9.14 | 2.12 起删除部分旧 API |
| ICU | 79.1 | 69.x | 新 ICU 与旧 intl 的兼容性需验证 |
| libzip | 1.12.1 | 1.7.x | PHP 7.4 的 zip 扩展接口较旧 |
| libcurl | 8.23.0 | 7.88.x | 与旧 OpenSSL 搭配更稳 |
| xdebug | 3.5.3 | 3.1.x | 新版本要求更高 PHP |
| swoole | 6.2.4 | 4.8.x | 6.x 要求 PHP 8.x |
| protobuf | 5.36.2 | 3.x | 新版本要求更高 PHP |

### 5.3 修改某个版本的 OpenSSL

| 影响范围 | 修改位置 |
| --- | --- |
| 所有版本 | `profiles/shared/deps/openssl-3.5.conf` 的内容，或共享片段的默认指向 |
| 8.5 整个系列 | `profiles/php-8.5/deps/openssl.conf` 改为 include 3.5 |
| 只有 8.5.12 | `profiles/php-8.5.12/deps/openssl.conf` 改为 include 3.5 |
| 只有当前安装根 | `<安装根>/configs/php-8.5.12.local.conf` 写 `OPENSSL_VERSION=3.5.9` |
| 固定本次构建 | `phprt config freeze --php 8.5.12` |

## 6. 安装根

### 6.1 现状

`install.sh` 把版本文件和 `config.conf` 拼成一份完整配置，写入 `<安装根>/configs/php-<版本>.conf`。新增 8.6 时会再产生一份完整副本。仓库里的修改不会自动同步，安装根里会出现多份需要人工维护的配置。

### 6.2 建议：指针 + 本地覆盖

```conf
# <安装根>/configs/php-8.5.12.conf
PHP_VERSION=8.5.12
PHP_PROFILE=php-8.5
PHPRT_REPO=/home/z/testphp/phprt
```

可选的本地覆盖：

```conf
# <安装根>/configs/php-8.5.12.local.conf
OPENSSL_VERSION=3.5.9
ENABLE_XDEBUG=0
```

构建时按以下顺序解析：

```text
仓库 profiles/shared/base.conf
  → 仓库 profiles/php-8.5/profile.conf
  → 仓库 profiles/php-8.5/deps/*.conf
  → 安装根 configs/php-8.5.12.local.conf
```

这样仓库是唯一配置源，安装根只记录「用哪个 profile」和「本机改了什么」。

### 6.3 快照作为显式操作

需要复现、离线归档或 CI 固定版本时：

```bash
phprt config freeze --php 8.5.12
```

生成一份完整配置和解析哈希，之后构建读这份 lock。默认不生成完整快照。

### 6.4 运行时不受影响

`./myphp` 只固化 `PHPRT_ROOT`、PHP 版本和 ini 路径，不读取 `configs/`。仓库移动或删除不会影响已经生成的项目入口，只影响后续的 `phprt build` 和 `phprt verify`。

## 7. 依赖约束与配方隔离

### 7.1 约束校验

版本范围应该作为数据保存，并在加载配置时校验：

```conf
# 示例，具体数值待实测
# 组件       PHP 范围    允许版本范围     理由
openssl      7.0..7.4    1.0.2..1.1.1    7.4 的 ext/openssl 不支持 3.x
openssl      8.0..8.9    1.1.1..3.9
libxml2      7.0..7.4    2.9.0..2.9.99   2.12 起删除旧 API
libxml2      8.0..8.9    2.9.0..2.99
```

越界时直接报错，并给出建议值。不要让用户在编译十分钟后才发现问题。

### 7.2 配方身份

`deps/<名字>-<版本>/` 只包含版本号，不包含编译参数和传递依赖。相同版本、不同配方会冲突：

- `deps/curl.sh` 的签名只记录了 `openssl=1`，没有 OpenSSL 版本。如果两个 PHP 版本共用同一个 curl 版本、但链接不同 OpenSSL，stamp 会复用错误产物。
- 即使强制重编，也是在原目录覆盖，另一个 PHP 版本的 rpath 会指向被改坏的库。

建议：

1. 依赖目录加入配方标记，例如 `DEP_FLAVOR=legacy74` → `deps/openssl-1.1.1w-legacy74/`。
2. 各依赖的 stamp 加入传递依赖版本，例如 `curl=8.23.0;openssl=3.5.9;zlib=1.3.2`。
3. `--prune` 继续依据 `.phprt-deps` 记录的实际路径保留，不做名称推断。

7.4 可以先统一使用 `DEP_FLAVOR=legacy74` 做软隔离；8.x 保持现有路径不变。

## 8. 扩展能力随版本变化

8.6 可能新增可选扩展、修改默认值，或把某个核心扩展移到 PECL。建议把「逻辑开关」和「实现」分开。

### 8.1 provider 声明

同一个开关可以有多份实现，按 PHP 范围选择：

```bash
# 示例：IMAP 从核心移到 PECL，具体版本以官方发布说明为准
# lib/modules/builtin/imap.sh
# phprt: name=imap switch=ENABLE_IMAP php=7.0..8.3 phase=builtin default=on

# lib/modules/ext/imap.sh
# phprt: name=imap switch=ENABLE_IMAP php=8.4.. phase=ext default=off needs=openssl
```

新增扩展只需要放一个文件：

```bash
# lib/modules/builtin/foo.sh
# phprt: name=foo switch=ENABLE_FOO php=8.6.. phase=builtin default=off
```

用户配置里没有 `ENABLE_FOO` 时使用 provider 的默认值；配置里写了但当前版本不支持时给出警告，`PHPRT_STRICT=1` 时升级为错误。

### 8.2 默认值与开关

建议的取值优先级：

```text
用户配置
  > 系列 profile
  > provider 的 default=
```

这样 8.6 新增的扩展不必写进每个版本的配置。

### 8.3 动态发现

要做到「加模块只丢文件」，还需要把以下硬编码改为发现式：

- 按目录确定 `phase`：`deps`、`builtin`、`ext`、`tools`、`app`。
- 按 `needs=` 做拓扑排序，替代固定的 `ALL_MODULES`。
- `enabled_module()` 由 provider 头部的 `switch=` 生成。
- `stamp_file()`、`dep_dir_of()` 由模块头部声明生成，或使用统一规则。
- `verify.sh` 从当前版本的 provider 集合生成期望扩展清单。

这一步比 provider 选择大，建议在 7.4 跑通后再做。

## 9. 工具

| 命令 | 用途 |
| --- | --- |
| `phprt modules --php 8.6` | 列出该版本实际存在的能力、选中的 provider、默认值 |
| `phprt config tree --php 8.5.12` | 打印继承链、每个依赖引用了哪个片段、哪些值被本地覆盖 |
| `phprt config explain --php 7.4 openssl` | 显示最终值、来源文件、是否超出允许范围 |
| `phprt config pin --php 8.5.12 openssl 3.5.0` | 在最近一层生成覆盖文件或修改 include 指向 |
| `phprt config freeze --php 8.5.12` | 生成完整快照和解析哈希，用于复现 |
| `phprt config doctor` | 检查断链、重复片段、delta 冗余、范围越界 |
| `phprt doctor --php 7.4` | 编译失败时归类错误，提示改 profile 还是加补丁 |

`config tree` 是这套方案的可调试性基础。没有它，目录叠加和 include 会很难排查。

## 10. 迁移步骤

### 阶段 0：基线测量

- 用 PHP 7.4.33 在本机 GCC 15 上跑一次 `./configure --help` 和最小构建。
- 确认是「加 CFLAGS 能过」还是「必须用旧工具链或容器」。
- 记录 OpenSSL 1.1.1w、libxml2 2.9.14、ICU 69 的实际编译结果。

退出条件：知道 7.4 的主要阻塞点，不要求编过。

### 阶段 1：加载器与 8.5 profile

- 实现 `extends`、`phprt_include`、最近同名文件覆盖、`config tree`。
- 把现有 8.5 配置拆成 `base.conf` + `php-8.5/profile.conf` + `deps/*.conf`。
- 保持 8.5 的 configure 参数和构建结果不变。

退出条件：`phprt config tree --php 8.5.12` 能还原现有配置；8.5 构建通过。

### 阶段 2：7.4 profile

- 新建 `profiles/php-7.4/`，加入依赖链接、参数覆盖、补丁。
- 加入版本范围校验和 `DEP_FLAVOR=legacy74`。
- 按 `phprt doctor` 的提示逐项修复。

退出条件：7.4 最小档能构建并通过 `phprt verify --php 7.4.33`。

### 阶段 3：8.6 与 provider 验证

- 新建 `profiles/php-8.6/`，只写差异。
- 选择一个「从核心移到 PECL」的扩展做 dry run，验证 provider 模型。
- 根据结果决定是否做动态发现。

退出条件：8.6 配置不超过 5 行；8.5 构建不受影响。

### 阶段 4：文档与工具

- 补充 `docs/ADD-PHP-VERSION.md`，说明目录、include、覆盖、校验和常见错误。
- 补充 `phprt config` 子命令文档。
- 在 README 的多版本章节加入新结构。

## 11. 风险与待确认

### 11.1 风险

1. 共享片段被误改。修改 `shared/deps/openssl-3.3.conf` 会影响所有引用者。需要在文件头提示，并由 `config tree` 标记 `(shared)`。
2. 7.4 的工具链风险。本机是 GCC 15.3.1、Bison 3.8.2，PHP 7.4 大概率需要额外 CFLAGS 或补丁；也可能最终必须用容器或旧 GCC。
3. 依赖版本组合不确定。表 5.2 是候选值，必须逐项实测，不能直接当作结论。
4. 安装根依赖仓库路径。指针模式要求仓库在构建时可达；需要 `freeze` 作为兜底。
5. 过度分层。目录和 include 层数过多会降低可读性。建议限制为「base → 系列 → 可选 patch 层」三层。
6. 磁盘占用。7.4 的依赖目录、补丁和 PHP 产物会额外占用空间；`DEP_FLAVOR` 会减少复用，这是有意的隔离成本。

### 11.2 待确认

1. `local.conf` 的层级：只放安装根，还是允许项目根再放一份优先于安装根？
2. `freeze` 是否需要在每次 `install.sh` 后自动生成，还是只在显式调用时生成？
3. provider 与动态发现是在 7.4 之前做，还是先跑通 7.4 再做？
4. 7.4 的扩展范围：最小集先跑通，还是与 8.5 全量对齐？
5. 7.4 是否允许使用容器或旧工具链，还是必须在本机 GCC 15 上编译？
6. 系列继承是否统一用 `extends`，还是坚持全部用 `include`？

## 12. 不在本方案范围内

- 具体补丁内容。7.4 的补丁需要在阶段 0 和阶段 2 根据实际错误生成。
- 8.6 正式版的最终变更清单。8.6 尚未发布，本文只描述机制。
- 把现有安装根迁移到新结构的具体脚本。迁移应在阶段 1 单独设计。
