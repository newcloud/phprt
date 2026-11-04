# phprt 项目本地自包含模型

- 状态：设计讨论稿，未实施
- 日期：2026-11-04
- 适用范围：phprt 的下一版组织方式
- 关系：`docs/PHP-VERSIONS.md` 描述的是「profile + 跨版本共享依赖」方向；本文是从最简手工流程反推的另一条路线。如果采用本文模型，以本文为准。

## 摘要

把 phprt 仓库当成「软件源 + 模板」，把项目里的 `.phprt` 当成「实例」。

`phprt set <版本>` 时，从仓库复制这个版本需要的脚本和源码到项目 `.phprt`，在项目内完成构建。之后运行、切换、增删扩展都只和 `.phprt` 打交道，不再读取 phprt 仓库。仓库只在安装新版本、补充未缓存的源码或显式刷新脚本时才需要。

这个模型没有全局安装根、没有跨版本共享依赖、没有 profile 继承、没有全局配置合并。每个项目的 `.phprt` 是一份完整、可搬走的实例。

## 1. 从手工流程反推

手工安装一个本地 PHP 环境只有五步：

1. 下载 PHP 和依赖源码；
2. 编译依赖库；
3. `configure` + `make` PHP；
4. 编译扩展 `.so`；
5. 写 `php.ini`，运行时设置 `LD_LIBRARY_PATH`。

变量只有四类：版本、编译方式（static / shared / 不编）、路径、编译和运行环境。

因此最小模型只有四个对象：

| 对象 | 作用 | 形态 |
| --- | --- | --- |
| recipe | 一个东西怎么编 | 一个 shell 脚本 |
| bundle | 一个 PHP 版本需要哪些 recipe、版本和顺序 | 一个目录 |
| tree | 编出来的自包含产物 | `php-<版本>/` |
| project | 当前用哪棵树、加载哪些扩展 | `.phprt/` + `myphp` |

## 2. 用户命令

```bash
phprt set php8.5.15       # 安装或切换；不存在时先选扩展并构建
phprt current             # 显示当前版本
phprt add ext sqlite      # 对当前树编译或启用扩展
phprt remove ext sqlite   # 停用扩展
phprt verify              # 检查依赖、扩展加载和是否链到系统库
```

可选的辅助命令：

```bash
phprt list                # 项目里已安装的版本树
phprt clean               # 删除 build/ 和未使用的源码缓存
```

## 3. `set` 流程

```text
phprt set php8.5.15
├─ 版本树已存在
│   ├─ 使用旧的
│   │   → 切 current
│   │   → 更新 myphp（如果 myphp 写死版本）
│   └─ 重新编译
│       → 进入扩展选择（预勾当前已选项）
│       → 重跑这棵树的 recipe
└─ 版本树不存在
    → 进入扩展选择
    → 计算源码闭包
    → 复制 runner、bundle、源码到项目 .phprt
    → 按阶段执行（deps → php → ext）
    → 成功后切 current，更新 myphp
```

### 3.1 扩展选择

选择列表来自当前 bundle 的扩展 recipe。每一项声明：

- 逻辑名称，例如 `sqlite`；
- 类型：`builtin-shared`、`pecl`、`static`；
- 编译进 PHP configure 的参数，或独立的 `phpize` 构建；
- 依赖的库；
- 默认是否勾选。

选择结果决定三件事：

1. PHP 的 `configure` 参数；
2. 需要额外构建的 PECL 扩展；
3. 需要复制或下载的源码闭包。

### 3.2 源码闭包

源码清单 = PHP 源码 + 选中扩展依赖的库源码 + PECL 扩展源码。

只复制选中集合需要的包，不复制整个 `downloads/`。同文件系统优先硬链接，跨文件系统才复制。

### 3.3 重新编译

重新编译只针对当前这棵树：

- 已有依赖目录和 stamp 可以复用；
- 先构建到 `php-<版本>.new`；
- 成功后替换 `current`，失败保留旧树；
- 不清空其他版本。

## 4. 项目目录

```text
blog/
├── myphp
└── .phprt/
    ├── bin/
    │   └── phprt                 # 本地入口，仓库删了也能用
    ├── bundles/
    │   └── php-8.5.15/           # 复制来的脚本和元数据
    ├── downloads/                # 本版本选中的源码包
    ├── build/                    # 解压目录，可删
    ├── php-8.5.15/               # 自包含树
    │   ├── bin/php
    │   ├── deps/                 # 这棵树自己的依赖
    │   ├── lib/php/extensions/   # 扩展 .so
    │   ├── etc/php.ini
    │   ├── BUILD-PLAN            # 选择结果和版本
    │   └── BUILD-INFO            # configure 行、依赖目录、时间
    ├── logs/
    ├── current -> php-8.5.15
    └── project.conf
```

依赖放在树内，而不是项目级 `deps/`，这样一棵树整体搬走也能用。多个版本各自带依赖，互不影响。

### 4.1 `BUILD-PLAN`

记录这棵树是怎么来的：

```conf
PHP_VERSION=8.5.15
PHP_URL="https://www.php.net/distributions/php-8.5.15.tar.xz"
EXTENSIONS="openssl zlib sqlite curl mbstring zip"
COMPILER="gcc 15.3.1"
BUILT_AT="2026-11-04T17:00:00+08:00"
```

### 4.2 `BUILD-INFO`

记录最终 `configure` 行、依赖目录和补丁列表，供 `add ext` 判断哪些扩展已经编过、哪些需要重新 `configure`。

## 5. 复制规则

| 内容 | 复制时机 | 说明 |
| --- | --- | --- |
| runner | 第一次 `set` | 通用执行器，复制到 `.phprt/bin/` |
| bundle | 每次 `set` 新版本 | 整个版本的 recipe 和元数据；脚本很小，一次复制完 |
| 源码 | `set` 时按闭包 | 只复制选中的；硬链接优先 |
| 树 | 本地构建 | 不复制，构建产生 |
| 其他版本 | 不复制 | 各自独立 |

复制源码后仍按 `SHA256SUMS` 校验。仓库路径记录在 `.phprt/config`，后续 `add ext` 缺源码时先去仓库 `downloads/` 找。

## 6. `add ext` 和 `remove ext`

### 6.1 `add ext`

```text
phprt add ext sqlite
  1. 读取 current 指向的树和 bundle
  2. 找到 sqlite 的 recipe
  3. 计算依赖闭包
  4. 缺源码时依次查找：
       .phprt/downloads/ → 仓库 downloads/ → 网络
  5. 按顺序构建缺少的依赖，再构建扩展
  6. 成功后写 conf.d/<编号>-<名字>.ini
  7. 失败不修改 ini
```

扩展可能带出新依赖。例如添加 `gd` 需要 libpng、jpeg、freetype，`add` 必须能递归构建，而不是只跑一个 `.so`。

### 6.2 `remove ext`

```text
phprt remove ext sqlite
  → 删除或注释 conf.d 里的 extension 行
  → .so 保留
  → 静态扩展无法移除，提示需要重新编译
```

默认不删除 `.so`，避免影响同一棵树里的其他项目入口。需要彻底清理时再加 `--purge`。

### 6.3 内置扩展没在 `set` 时选中

`add` 可能需要在 PHP 源码目录重新 `configure` 并 `make`。保留 `downloads/php-*.tar.*` 即可重新解压；`build/` 可以删除。

## 7. 版本切换

`current` 是符号链接：

```text
.phprt/current -> php-8.5.15
```

`myphp` 指向 `.phprt/current/bin/php`。切换版本只改链接，`myphp` 不需要改。

如果希望 `myphp` 里明文写版本号，可以在切换时同时更新 `PHPRT_PHP` 和 `PHPRT_ROOT`。两种方式二选一。

## 8. 自包含边界

| 操作 | 仓库不在时 |
| --- | --- |
| 运行 `./myphp`、`phprt current`、`phprt verify` | 正常 |
| 切到已存在的版本 | 正常 |
| `add ext`，源码在本地 | 正常 |
| `add ext`，源码不在本地 | 能联网就下载，离线则失败 |
| `set` 一个从没复制过的新版本 | 需要仓库 |
| 更新 bundle 脚本 | 需要仓库，走显式刷新 |

仓库是软件源，不是运行时依赖。

## 9. 边界情况

1. `add ext` 可能引入新的依赖库，必须递归构建。
2. 静态扩展不能移除，只能重新编译。
3. 切换版本时，新树可能缺项目 ini 里的扩展；`set` 应检查并提示。
4. 每个项目一棵树会占磁盘；源码用硬链接缓解，树不能共享。
5. `.phprt/downloads/`、`build/`、`php-*/`、`logs/` 不应进 git；`myphp`、`project.conf`、`bundles/` 可以进。
6. 脚本复制后是冻结的；仓库改进不会自动生效，需要显式刷新。
7. 同一项目里切换版本，`project.conf` 的扩展列表可能和树不匹配；建议按版本记录选择结果，切换时取交集并提示缺失项。

## 10. 与当前架构的对比

| 当前 | 新模型 |
| --- | --- |
| 全局安装根，多个项目共用 | 每个项目自己的 `.phprt` |
| 依赖跨版本共享 | 每个版本树自带依赖 |
| `configs/php-<版本>.conf` | 项目 `.phprt/project.conf` + 树内 `BUILD-PLAN` |
| 全局 `config.conf` 合并 | bundle 自带版本和参数 |
| `lib/modules` 跨版本复用 | bundle 内各自一套 recipe |
| stamp 判断是否重装 | 树和 `.so` 是否存在 + `BUILD-PLAN` |
| `install.sh` / `phprt init` / `phprt build` | `phprt set` / `add ext` / `remove ext` |
| 项目只选版本和 ini | 项目还决定编译哪些扩展 |

## 11. 新版本 phprt 仓库的样子

### 11.1 目录结构

```text
phprt/
├── phprt                        # 入口：仓库里跑，或复制到项目 .phprt/bin/
├── runner/
│   ├── build.sh                 # 解析选择，按 deps → php → ext 执行
│   ├── commands.sh              # set / current / list / add / remove / verify
│   ├── common.sh                # 下载、校验、解压、日志、wheel 化
│   ├── myphp.tmpl               # 项目入口模板
│   └── vendor/                  # perl 核心模块垫片
├── bundles/
│   ├── php-8.5.12/
│   │   ├── bundle.conf          # 版本、源码、默认扩展
│   │   ├── php.sh               # 根据选择生成 configure 参数
│   │   ├── builtin/*.sh         # 内置扩展（编进 PHP 本体）
│   │   ├── deps/*.sh            # 依赖库 recipe
│   │   └── ext/*.sh             # PECL 扩展 recipe
│   ├── php-8.6.0-dev/
│   └── php-7.4.33/
├── downloads/                   # 源码缓存，作为项目的种子（tarball 不进 git）
├── docs/
└── LICENSE
```

### 11.2 各目录的职责

| 路径 | 职责 | 是否复制到项目 |
| --- | --- | --- |
| `phprt` | 入口：仓库里直接跑，复制到项目后靠 `.phprt/config` 找回仓库 | 是（复制为 `.phprt/bin/phprt`） |
| `runner/` | 通用执行逻辑，不含 PHP 版本知识 | 是 |
| `bundles/<版本>/` | 该版本的全部版本、参数和 recipe | 是，整套复制 |
| `downloads/` | 源码种子缓存 | 按闭包复制或硬链接 |
| `docs/` | 使用和 bundle 编写说明 | 否 |

### 11.3 bundle 内容

`bundle.conf`：

```conf
PHP_VERSION=8.5.12
PHP_URL="https://www.php.net/distributions/php-8.5.12.tar.xz"
PHP_SHA256=""
PHP_SRC_DIR="php-8.5.12"
DEFAULT_EXTS="openssl zlib libxml2 curl sqlite mysqli mbstring zip"
SUPPORTED_EXTS="openssl zlib libxml2 curl sqlite mysqli mbstring intl zip gd ffi redis protobuf xdebug apcu yaml swoole"
```

不再手写固定 `order`。runner 根据选择算出依赖闭包和拓扑顺序，写入 `<tree>/BUILD-ORDER`：

```text
deps/openssl
deps/zlib
deps/libxml2
deps/curl
deps/sqlite
deps/oniguruma
deps/libzip
php
ext/redis
ext/swoole
```

`php.sh` 只生成 `configure` 参数，不在里面建依赖。例如：

```bash
# 选中 sqlite 时
ARGS+=(--with-sqlite3=shared --with-pdo-sqlite=shared)
# 选中 curl 时
ARGS+=(--with-curl="$DEPS/curl-8.23.0")
```

`deps/*.sh` 自己带版本、URL、SHA256 和 `configure` 参数。`ext/*.sh` 带扩展类型、依赖和构建方式。

### 11.4 bundle 的生成方式

- 新补丁版本：复制上一个 bundle，改 `bundle.conf` 和确实变化的依赖版本。
- 新开发分支：复制相近 bundle，改 PHP 源码、参数和扩展版本。
- 不跨 bundle 继承。重复是故意的，换来确定性。
- 可提供一个辅助命令 `phprt bundle new <版本> --from <版本>`，只做复制和改版本号。
- `phprt bundle check` 检查依赖、源码 URL、SHA256 是否齐全。

### 11.5 runner 的边界

`runner/` 只负责：

- 解析参数；
- 发现 `.phprt`；
- 运行选择界面；
- 复制 bundle 和源码；
- 按阶段和依赖顺序调用脚本；
- 维护 `current`、日志和 `myphp`；
- 校验源码和运行结果。

它不包含任何具体 PHP 版本、依赖版本或扩展参数。那些全部在 bundle 里。

### 11.6 增量构建

构建分四段：

```text
resolve → deps → php → ext
```

`order` 文件不再是唯一调度来源。runner 先根据选择算出内置扩展、PECL 扩展和依赖闭包，再做拓扑排序，写入 `<tree>/BUILD-ORDER`。

每段有自己的签名：

| 步骤 | 签名内容 | 什么时候重跑 |
| --- | --- | --- |
| 依赖 | recipe + 版本 + 传递依赖 + 相关开关 | 自己的版本或 recipe 变了，或它的依赖变了 |
| PHP | 最终 configure 行 + `php.sh` + `builtin/*.sh` + PHP 用到的依赖签名 | configure 行或 PHP 用到的依赖变了 |
| PECL | ext recipe + PHP ABI 签名 + 自己的依赖签名 | 扩展本身、PHP ABI 或它的依赖变了 |

签名写在 `<tree>/.phprt-state/`。规则：

- 签名一致且产物存在 → 跳过；
- 失败不写签名，但保留源码、对象文件和已完成的签名；
- 重试从失败处继续；
- `--recompile` 只清 PHP 签名，依赖和扩展仍按签名判断；
- `--clean-src` 清源码和全部签名，下一次是完整构建；
- 源码已解压就复用，只有 `--clean-src` 才重新解压。

`set --plan` 只打印 `deps / php / ext` 计划，不构建。

## 12. 原型建议

原型已经覆盖了最小闭环：

1. 三个 bundle：`php-8.5.12`、`php-8.6.0-dev` 和 `php-7.4.33`
   （7.4 目前是最小档：`libxml2` + 内核基础扩展）。
2. 内置扩展：`openssl zlib libxml2 curl sqlite mysqli mbstring intl zip gd ffi`。
3. PECL 扩展：`redis protobuf xdebug apcu yaml swoole`。
4. 实现了 `set`、`set --plan`、`current`、`list`、`add ext`、`remove ext`、`verify`。
5. 验证了：
   - 复制脚本和源码后，项目能在本地构建；
   - `set` 已有版本只切链接，不重编；
   - 依赖、PHP、扩展按签名跳过；第二次相同选择不产生编译；
   - 只加 PECL 扩展时跳过 PHP；
   - 失败后已完成步骤保留，重试从失败处继续；
   - `php-7.4.33` 最小档构建通过，libxml2 2.9.14 来自树内，`ldd` 没有系统库；
   - `verify` 确认没有链接系统库。

后续按下面的顺序扩展：

1. 验证 `gd` / `intl` 的递归依赖和内置扩展重编。
2. 给 `php-7.4.33` 增加内置扩展和 PECL（OpenSSL 1.1.1w、旧版 redis/xdebug 等）。
3. 旧依赖清理和源码缓存的显式管理。
4. 把扩展启用状态按版本分开，避免切换版本时互相删 `conf.d` 的 ini。

## 13. 待确认

1. `set` 是否总是项目本地，还是保留 `--global` 写入 `~/.phprt`？
2. `set` 时复制整个 bundle，还是只复制选中扩展的 recipe？建议整个 bundle。
3. `add ext` 是否允许自动拉取新的依赖库？建议允许，否则 `gd` 这类扩展无法后加。
4. `current` 用符号链接，还是每次改写 `myphp` 的版本行？
5. 全局 `phprt` 命令是否只是薄入口，实际执行复制到项目的 `runner/phprt`？
6. `.phprt` 的 git 忽略范围如何定？
7. bundle 脚本更新用 `set --refresh` 还是单独的 `phprt bundle update`？
8. 旧仓库代码是原地重构，还是新建 `v2/` 并行验证？
