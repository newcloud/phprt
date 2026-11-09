# 扩展覆盖计划（phprtv2 / PHP 8.5）

本文是 phprtv2 里**唯一**的扩展覆盖进度表。`phprt`（v1）仓库的
`docs/EXTENSIONS.md` 只作历史参考，不再更新：两边的目录、driver 名和契约都不一样，
双写必然漂移。

目标：把 php8.5 的扩展覆盖面补齐到 v1 的水平，再完成 v1 计划表里剩余的常用扩展。
默认安装的版本必须是**安全版本**，测试要能证明“旧版 → 安全版”的升级路径可用。

本文的进度、问题、优化提案都随开发过程实时更新，不等到收尾。

## 1. 现状

| 类别 | v1 | v2 | 缺 |
| --- | --- | --- | --- |
| builtin | 17 | 3 | 14 |
| ext（PECL） | 8 | 1 | 7 |
| deps | 20 | 4 | 16 |
| tools | 3 | 2 | 1 |
| 合计 | 48 | 10 | 38 |

v2 现有的 10 个：builtin `curl openssl zip`；ext `swoole`；deps `curl libzip openssl zlib`；
tools `autoconf perlshim`。

## 2. v2 当前约束

这一节是硬条件：加扩展、改 driver 都必须满足；不满足的先按第 8 节的方式停下讨论。

| # | 约束 | 依据 |
| --- | --- | --- |
| 1 | **只做 PHP 8.5**。v2 只有 `bundles/php8.5`，8.6 / 7.4 未迁移 | `bundles/` |
| 2 | **`bundles/` 是数据，`runner/` 是驱动**；driver 里不出现具体包名分支 | `docs/ROADMAP-V2.md` 第 1 节 |
| 3 | **契约 v2**：builtin 写 `flags()` / `needs()`，ext 写 `build()` / `enable()`，dep / tool 写 `build()`；末行必须是 `recipe_main "$@"` | `docs/RECIPE.md` |
| 4 | **门禁**：`scripts/recipe-check.sh --deep` 必须通过。它是开发脚本，不挂 `phprt` 子命令 | 本轮已定 |
| 5 | **版本源在 `bundle.conf`**：recipe 头部只写变量名（`version=LIBZIP_VERSION`）。Phase C（版本下沉到 recipe）未做 | `bundle.conf` |
| 6 | **目录**：dep → `<tree>/deps/<名>-<版本>`；tool → `.phprt/build-php-<版本>/tools/`；builtin / ext → PHP 树。`.phprt` 顶层只允许 `current`、`php-*`、`build-php-*`、`buildroots` | `AGENTS.md` 17 |
| 7 | **没有 `SUPPORTED_EXTS`**：扩展清单就是 `{builtin,ext}/*.sh` 的文件名 | `common.sh:builtin_names` |
| 8 | **扩展状态不落盘**：启用状态由 `php -m` 推导，不写 `project.conf`；用户删 ini 或 `.so` 后结果仍要准 | `AGENTS.md` 16 |
| 9 | **ini 必须显式**：builtin 静态写 `ini=-`，共享模块写 `20-<名>.ini`；ext 必须实现 `enable()` | `RECIPE.md` 3.1 |
| 10 | **wheel 化不手写 ELF**：库放 `lib/` 或 `lib64/` 根目录，真实文件是 `.so.N` 且 SONAME 带 ABI 号；例外只用 `wheel=none/exclude/alias/singleton` | `RECIPE.md` 6 |
| 11 | **不写系统目录、不引入第二份 CA**；CA 固定在 `etc/ssl/cert.pem` | `AGENTS.md` 9/10 |
| 12 | **重编代价**：改 builtin 会改 PHP 签名 → PHP 全量重编；dep / ext 只重编自己 | `build.sh` |
| 13 | **下载/离线**：源码包优先仓库 `downloads/`（当前软链到 v1 的），缺的进 `build-php-<版本>/downloads/`；`OFFLINE=1` 是全局离线开关 | `common.sh:find_source` |
| 14 | **尚未实现**：`phprt update`、install / add 的在线检查（M2）、`--no-update-check`、`--offline` 参数 | 代码现状 |
| 15 | **所有 pin 都填了真实 `*_SHA256`**（PHP 与全部依赖/扩展），下载即校验 | 2026-11-09 回填 |
| 16 | **v1 / v2 不能共用一个 `.phprt`**，会互相覆盖。测试项目分开：`v2demo` 最小集、`v2ext` 扩展开发集 | 实测 |
| 17 | **暂缓/不做**：imap、grpc、mcrypt、odbc、snmp、pspell、dba、solr、oauth、readline、sqlsrv、oci8 | v1 文档 |

## 3. 三类节点与命名

| 类型 | 路径 | 产物 | 契约函数 |
| --- | --- | --- | --- |
| builtin | `bundles/php8.5/builtin/<名>.sh` | 编进 `bin/php`，或 `lib/php/extensions/*/<名>.so` | `flags()` / `needs()` |
| ext | `bundles/php8.5/ext/<名>.sh` | `lib/php/extensions/*/<名>.so` + `etc/conf.d/<ini>` | `build()` / `enable()` |
| dep | `bundles/php8.5/deps/<库>.sh` | `<tree>/deps/<库>-<版本>/` | `build()` |
| tool | `bundles/php8.5/tools/<工具>.sh` | `build-php-<版本>/tools/<工具>-<版本>/` | `build()` |

约定：

- 开关名不要求等于 `ENABLE_<NAME>`（`mbstring` 是 `ENABLE_ONIGURUMA`，`zip` 是 `ENABLE_LIBZIP`）。
- 版本变量名不能按扩展名猜（`curl` → `LIBCURL_VERSION`，`yaml` → `YAML_EXT_VERSION`）。
- ini 编号沿用现状，不要新建默认值：`20-redis`、`23-protobuf`、`25-apcu`、`26-yaml`、
  `27-swoole`、`30-xdebug`、`20-zlib`、`20-sqlite`。
- `needs` / `optional` / `tools` 即使为空也要写。

## 4. 迁移清单（38 个）

状态：`⬜ 待迁移`、`🚧 进行中`、`✅ 完成`、`⏸ 暂缓`、`❌ 放弃`。
`升级增量`：`有`、`无（首个 8.5 版本）`、`无（builtin）`、`待定`。

基线 = 当前 pin；目标 = 同系列下一个安全 patch（批次开工时按上游确认，不跨小版本）。

### 4.1 依赖与工具（17）

> 2026-11-09：17 个库全部构建成功，SONAME 均有 ABI 号；有安全更新的
> `libpng` / `aom` / `jpeg` 已完成“基线 → 目标”升级轮（在 v2up 副本里做，
> 基线树保留）。其余模块当前即最新。

| 名称 | 类型 | 依赖 | 版本变量 | 基线 | 目标 | 升级增量 | 状态 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| re2c | tool | — | `RE2C_VERSION` | 4.6 | 4.6 | 无（当前即最新） | ✅ |
| aom | dep | — | `AOM_VERSION` | 3.12.1 | 3.12.1 | 无（当前即最新） | ✅ |
| avif | dep | aom | `AVIF_VERSION` | 1.3.0 | 1.3.0 | 无（当前即最新） | ✅ |
| bzip2 | dep | — | `BZIP2_VERSION` | 1.0.8 | 1.0.8 | 无（当前即最新） | ✅ |
| freetype | dep | zlib, libpng | `FREETYPE_VERSION` | 2.13.3 | 2.13.3 | 无（当前即最新） | ✅ |
| icu | dep | — | `ICU_VERSION` | 79.1 | 79.1 | 无（当前即最新） | ✅ |
| jpeg | dep | — | `JPEG_VERSION` | 3.1.4.1 | 3.1.4.1 | 无（当前即最新） | ✅ |
| libffi | dep | — | `LIBFFI_VERSION` | 3.4.8 | 3.4.8 | 无（当前即最新） | ✅ |
| libpng | dep | zlib | `LIBPNG_VERSION` | 1.6.59 | 1.6.59 | 无（当前即最新） | ✅ |
| libsodium | dep | — | `LIBSODIUM_VERSION` | 1.0.22 | 1.0.22 | 无（当前即最新） | ✅ |
| libxml2 | dep | zlib | `LIBXML2_VERSION` | 2.15.4 | 2.15.4 | 无（当前即最新） | ✅ |
| libxslt | dep | libxml2 | `LIBXSLT_VERSION` | 1.1.45 | 1.1.45 | 无（当前即最新） | ✅ |
| libyaml | dep | — | `LIBYAML_VERSION` | 0.2.5 | 0.2.5 | 无（当前即最新） | ✅ |
| oniguruma | dep | — | `ONIGURUMA_VERSION` | 6.9.10 | 6.9.10 | 无（当前即最新） | ✅ |
| openldap | dep | — | `OPENLDAP_VERSION` | 2.6.16 | 2.6.16 | 无（当前即最新） | ✅ |
| sqlite | dep | — | `SQLITE_VERSION` | 3.54.1 | 3.54.1 | 无（当前即最新） | ✅ |
| webp | dep | — | `WEBP_VERSION` | 1.6.0 | 1.6.0 | 无（当前即最新） | ✅ |

### 4.2 PECL 扩展（7）

> 2026-11-09：7 个 PECL 全部用 `phprt add ext` 增量构建并做了功能调用。
> `apcu` / `igbinary` 先按自包含写法验证（82 / 88 行），归一化后只差 18 行，
> 据此加了 `runner/recipe-ext.sh`；两者改成助手版后降到 22 / 31 行并重新构建通过。
> `igbinary` 的 3.2.17RC1 被预发布过滤挡掉，当前 3.2.16 就是最新稳定版。

| 名称 | 依赖 | 版本变量 | ini | 基线 | 目标 | 升级增量 | 状态 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| apcu | — | `APCU_VERSION` | `25-apcu.ini` | 5.1.28 | 5.1.28 | 无（当前即最新） | ✅ |
| igbinary | — | `IGBINARY_VERSION` | `20-igbinary.ini`（待确认） | 3.2.16 | 3.2.16 | 无（3.2.17RC1 已过滤） | ✅ |
| msgpack | — | `MSGPACK_VERSION` | `21-msgpack.ini`（待确认） | 3.0.1 | 3.0.1 | 无（当前即最新） | ✅ |
| protobuf | — | `PROTOBUF_VERSION` | `23-protobuf.ini` | 5.36.2 | 5.36.2 | 无（当前即最新） | ✅ |
| redis | — | `REDIS_VERSION` | `20-redis.ini` | 6.3.0 | 6.3.0 | 无（当前即最新） | ✅ |
| xdebug | zlib | `XDEBUG_VERSION` | `30-xdebug.ini` | 3.5.3 | 3.5.3 | 无（当前即最新） | ✅ |
| yaml | libyaml | `YAML_EXT_VERSION` | `26-yaml.ini` | 2.3.0 | 2.3.0 | 无（当前即最新） | ✅ |

> igbinary / msgpack 在 v1 里写 ini 的位置需要迁移时核对（v1 审计表没有列出），
> 以 v1 recipe 的实际文件名为准，并同步更新本表。

### 4.3 builtin（14）

> 2026-11-09：14 个 recipe 全部编译成功，功能调用 14 项全部通过
> （`intl` 因首次 `--ext` 列表漏写，补编一次后通过），搬移测试通过。
> builtin 的版本跟 PHP 走，升级增量按“关闭 → 开启”验证。

| 名称 | 依赖 | modules | ini | 升级增量 | 状态 |
| --- | --- | --- | --- | --- | --- |
| bz2 | bzip2 | bz2 | - | 无（builtin） | ✅ |
| ffi | libffi | ffi | - | 无（builtin） | ✅ |
| ftp | — | ftp | - | 无（builtin） | ✅ |
| gd | zlib, libpng, jpeg, freetype, webp, aom, avif | gd | - | 无（builtin） | ✅ |
| gettext | —（glibc） | gettext | - | 无（builtin） | ✅ |
| intl | icu | intl | - | 无（builtin） | ✅ |
| ldap | openldap | ldap | - | 无（builtin） | ✅ |
| libxml2 | libxml2 | dom,xml,simplexml,xmlreader,xmlwriter,soap | - | 无（builtin） | ✅ |
| mbstring | oniguruma | mbstring | - | 无（builtin） | ✅ |
| mysqli | —（mysqlnd） | mysqli,pdo_mysql | - | 无（builtin） | ✅ |
| sodium | libsodium | sodium | - | 无（builtin） | ✅ |
| sqlite | sqlite | sqlite3,pdo_sqlite | - | 无（builtin） | ✅ |
| xsl | libxslt | xsl | - | 无（builtin） | ✅ |
| zlib | zlib | zlib | - | 无（builtin） | ✅ |

> builtin 的版本跟 PHP 走，没有独立的“降级/升级”。等价验证是：关闭时 `php -m` 里没有，
> 开启并重编后有；PHP 自身升级单独做一轮（8.5.11 → 8.5.12）。

## 4.4 编译方式（static / shared）

2026-11-09 定稿：

- `builtin/` / `ext/` 只表示源码来源，不再决定编译方式。
- 每个 builtin/ext recipe 头部必须写：
  - `type=static|shared|static,shared`
  - `default=static|shared`（必须出现在 `type` 里）
- `install` 不带参数时用 `default`；`--build "a=static,b=shared"` 逐项覆盖
  （也支持重复 `--build a=static --build b=shared`）；
  `--static` / `--shared` 全局指定，类型不支持的保持原方式并提示。
- `install --static-all` 是独立的全静态链接模式：
  - 扩展和第三方依赖都链接进 `bin/php`；
  - `type=shared` 的扩展自动排除；
  - glibc 仍动态，产物可单独复制运行；
  - 详见 [STATIC-ALL.md](STATIC-ALL.md)。
- `install --static-all-lto` 在 `--static-all` 基础上给 PHP 本体和依赖都开
  `-flto=auto`；状态记录在 `.phprt-state/lto`，详见
  [STATIC-ALL-LTO.md](STATIC-ALL-LTO.md)。
- `add ext` 忽略 `default`，必须显式带 `--shared` 或 `--static`；
  不带参数时打印 `type` / `default` 后退出。
- 编译方式记录在 `$TREE/.phprt-state/mode/<名字>`，切换方式会重编 PHP：
  - 未安装 + shared：只编 `.so`，PHP 不动。
  - 未安装 + static：重编 PHP。
  - static ↔ shared：必须重编 PHP。

判定（2026-11-09）：

| 类别 | recipe | type | default |
| --- | --- | --- | --- |
| builtin 单模块 | curl ffi ftp gd gettext gmp intl ldap mbstring sodium tidy xsl zip | `static,shared` | `static` |
| builtin 核心耦合 | bz2 openssl zlib | `static` | `static` |
| builtin 多模块 | libxml2 mysqli pgsql sqlite | `static` | `static` |
| PECL 普通 | amqp apcu igbinary imagick memcached mongodb msgpack protobuf rdkafka redis ssh2 yaml | `static,shared` | `shared` |
| PECL 只 shared | swoole xdebug | `shared` | `shared` |

- bz2 / openssl / zlib 只 static：phar 的 bz2、OpenSSL 签名、gzip 依赖编译期开关，
  改成 shared 会静默丢失这些能力。
- libxml2 / mysqli / pgsql / sqlite 暂只 static：一个 recipe 对应多个 `.so`，
  当前构建器只产一个模块。
- zlib / sqlite 原先默认 shared，现在默认 static；默认安装的产物从 `.so`
  改成编入 PHP。空项目默认安装已验证：11 个默认扩展全部 static，
  `lib/php/extensions/` 不存在，`Phar::canCompress(Phar::GZ)` 仍为 true，
  `phprt verify` 全绿。
- 全 static 的树没有 `lib/php/extensions/`，`enable_ext` 和后续 `add ext`
  必须容忍这个目录不存在。

`--static-all` 的支持范围按阶段扩大：

| 阶段 | 范围 | 状态 |
| --- | --- | --- |
| v0 | `openssl zlib curl sqlite mbstring` | 已实现（2026-11-09） |
| LTO v0 | `--static-all-lto`：PHP + `openssl zlib curl sqlite oniguruma` | 已实现（2026-11-09） |
| v1 | `libxml2` 已接入；`libzip`、`libsodium` 待做 | 部分完成（2026-11-09） |
| v2 | PECL：redis、yaml、ssh2、apcu、msgpack、igbinary、protobuf、amqp、memcached、mongodb、rdkafka | 待做 |
| v3 | intl/ICU、gd 图片链、xsl、ldap、pgsql、gmp、ffi、tidy | 待做 |
| v4 | imagick | 待做 |

注意：`--static` 仍是“扩展静态、依赖动态”；需要单个可复制文件时用
`--static-all`。两者都会重编 PHP。

已知构建细节：

- static PECL 把源码放进 `php-src/ext/<名字>` 后必须 `buildconf --force`
  重新生成 configure；否则 PHP 报 `unrecognized options: --enable-redis`。
- static 集合变化后必须 `make distclean`；否则旧的 `main/internal_functions_cli.dep`
  还引用着已删除的 `ext/<名字>/...h`，make 报“没有规则可制作目标”。
- shared builtin 的 phpize 构建要带上树内依赖的 `CPPFLAGS` / `LDFLAGS` /
  `PKG_CONFIG_PATH`；否则 intl 会链接系统 ICU。
- `--static-all` 下不能依赖 `-Wl,-Bstatic`：libtool 会把它重排到对象文件
  之前。实际做法是只含 `.a` 的 `static-libs/` 搜索目录 + 重复 `EXTRA_LIBS`；
  同时必须删掉依赖目录的绝对 rpath。
- 依赖只有 `.so` 时 `--static-all` 会在链接阶段失败；补齐 `.a` 属于 v1 之后的
  增量工作，不在 v0 范围。
- 依赖开 LTO 时不能统一覆盖 `CFLAGS`，否则会丢掉 recipe 原有的 `-O2`/`-O3`；
  做法是保留基线再追加 `-flto=auto`，并用 `gcc-ar`/`gcc-ranlib` 生成 `.a`。
  构建后检查 `.a` 里的 `.gnu.lto` 段，避免 recipe 漏接 LTO。

## 5. 新增清单（9 项，v1 计划表剩余）

| 扩展 | 类型 | 新依赖 | PHP 8.5 支持 | configure 参数 | 基线 | 目标 | 升级增量 | 状态 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| gmp | builtin | gmp 6.3.0 | 8.5 编译通过 | `--with-gmp=<deps/gmp>` | — | — | 无（builtin） | ✅ |
| tidy | builtin | tidy-html5 5.8.0 | 8.5 编译通过 | `--with-tidy=<deps/tidy>` | — | — | 无（builtin） | ✅ |
| ssh2 | ext | libssh2 1.11.1 | 8.5 编译通过 | `--with-ssh2=<deps/libssh2>` | 1.5.0 | 1.5.0 | 无（首个 8.5 版本） | ✅ |
| pgsql + pdo_pgsql | builtin | libpq 18.6 | 8.5 编译通过 | `--with-pgsql=<deps/libpq>`、`--with-pdo-pgsql=` | — | — | 无（builtin） | ✅ |
| memcached | ext | libmemcached-awesome 1.1.4 | 8.5 编译通过 | `--with-libmemcached-dir=`、`--disable-memcached-sasl` | 3.4.0 | 3.4.0 | 无（首个 8.5 版本） | ✅ |
| mongodb | ext | 自带 libmongoc/libbson | 8.5 编译通过 | `--with-mongodb-zstd=no --with-mongodb-snappy=no` | 2.5.7 | 2.5.7 | 无（首个 8.5 版本） | ✅ |
| amqp | ext | rabbitmq-c 0.18.1 | 8.5 编译通过 | `--with-librabbitmq-dir=` | 2.2.0 | 2.2.0 | 无（首个 8.5 版本） | ✅ |
| imagick | ext | ImageMagick 7.1.2-35 | 8.5 编译通过 | `--with-imagick=` + 关 OpenMP | 3.8.1 | 3.8.1 | 无（首个 8.5 版本） | ✅ |
| rdkafka | ext | librdkafka 2.16.0 | 8.5 编译通过 | `--with-rdkafka=`、`--disable-gssapi` | 6.0.5 | 6.0.5 | 无（首个 8.5 版本） | ✅ |

“待查”的项在对应批次开工前补齐，写进本表，不留在代码注释里。

## 6. 验收标准（分级）

| 类型 | 必过项 |
| --- | --- |
| 迁移的 dep / tool | 构建 + SONAME / wheel 检查 + `recipe-check` |
| 迁移的 ext | 构建 + `php -m` + 一个功能调用 + `verify` |
| 迁移的 builtin | 构建 + `php -m`（多模块逐个确认）+ 一个功能调用 + `verify` |
| 新增扩展 | 构建 + `php -m` + 功能调用 + `verify` + 搬移测试 |
| 每批结束 | `scripts/recipe-check.sh --deep`、`phprt verify` 全绿、commit（不 push） |

## 7. 添加一个扩展的流程

### 阶段 0：登记与决策（文档先行）

1. 在本文件第 4 / 5 节加行，状态改 `🚧`。
2. 定安全版本：上游最新稳定 + 声明支持 PHP 8.5 + 无未修复高危 CVE，记录来源链接。
3. 定基线版本：优先 v1 用过的版本；没有就选同系列上一个 patch。
4. 若首个支持 8.5 的版本就是安全版本，标 **`无升级增量`**，只做同版本重装 + 功能验证，
   不硬找更老的 tag（A 方案）。
5. 列出依赖闭包，标出 `optional`（如 gd 的图片格式开关）。

### 阶段 1：依赖先行

1. `bundle.conf` 加 `<LIB>_VERSION` / `_URL` / `_SHA256`；新模块的 SHA256 必须是真值。
2. 写 `deps/<库>.sh`（`contract=v2`，`build()`）。
3. 基线构建 → SONAME / wheel 检查。
4. 升级到目标版本 → 再构建 → 对比前后 `wheel.map`，确认 ABI 变化范围。
5. `scripts/recipe-check.sh`。

### 阶段 2：扩展 recipe

- builtin：`flags()` / `needs()`，`modules` 写全，`ini` 显式，`type` / `default` 必填。
- ext：`build()` + `enable()`；要支持 static 时加 `stage_static()` 和 `static_ldflags()`。
- 静态检查通过后才能开编。

### 阶段 3：基线安装（开发轮）

1. 独立项目 `v2ext`，从空白 `.phprt` 装。
2. 用基线 pin 装：`phprt install 8.5.12 --ext "<依赖+扩展>"`，或增量 `phprt add ext <名>`。
3. 验收：`php -m`、功能调用、**未启用时不出现**、`verify`、搬移。

### 阶段 4：升级验证（安全轮，在副本里做）

1. 复制项目：`cp -a v2ext v2ext-upgrade`，基线树保持可回退。
2. 把 pin 从基线改到目标版本。
3. 重装：M2 之前用 `install --recompile` / `add ext` 覆盖；M2 之后改用 `phprt update`
   并验证 `UPDATE-HISTORY`。
4. 验收：
   - `php -m` 与基线一致；
   - 功能调用通过；
   - `verify` 全绿；
   - `wheel.map`、`BUILD-INFO` 反映新版本；
   - 旧版本树 / 旧构建区仍在；
   - 记录升级前后的版本和耗时。
5. 升级失败：保留副本现场，修好再重跑；基线项目不动。

### 阶段 5：收尾

1. 回填状态、日期、基线 / 目标版本、验证项。
2. 在本文件第 10 节“实测记录”追加命令和结果摘要；失败和放弃的行保留原因。
3. `docs/TEST-V2.md` 追加同批结论。
4. commit（不 push），信息里带扩展名。

### builtin 的专门规则

| 轮次 | 等价操作 |
| --- | --- |
| 基线 | 关闭该 builtin 编一次，`php -m` 里没有它 |
| 升级 | 开启后重编 PHP，`php -m` 里出现它 |
| 多模块 | `libxml2` / `sqlite` / `mysqli` 每个模块都要确认 |
| PHP 自身 | 单独一轮：8.5.11 → 8.5.12（两棵树可以并存） |

## 8. 开发方式

- 目标是**可用的扩展覆盖**，不是把清单勾完。
- 遇到下面三类情况，**停止、报告、等决定**，不擅自实施：
  1. 框架 / 流程问题（签名、resolver、wheel、契约校验、缺能力）；
  2. 新的优化方向（减少重编、加速、简化、提高校验覆盖）；
  3. 需要改 `runner/` 才能通过的情况。
- 扩展内部问题（参数、patch、上游编译错误、版本不支持）自行处理并记台账，不打断。
- 允许中途重构、调整批次、放弃某项；放弃要写原因。
- 每次停下记录：当时在做什么、发现了什么、可选方案、我的建议。

## 9. 问题台账

| 日期 | 模块 | 分类 | 现象 | 处理 | 状态 | 影响框架 |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-11-09 | libxml2 / sqlite | 框架 | 同名 builtin/dep，`ext_modules` 取到 dep，报“没有编译” | 加 `ext_recipe_sh`（builtin → ext） | 已修复 | 是 |
| 2026-11-09 | igbinary | 框架 | PECL 的 `3.2.17RC1` 被标成 stable，归一化后变成 `3.2.17`，下载 404 | PECL 解析丢弃 RC/alpha/beta；3.2.16 即最新 | 已修复 | 是 |
| 2026-11-09 | gmp | 扩展内部 | GMP 6.3.0 的编译器探测程序依赖隐式函数声明，GCC 15 默认 gnu23 报“could not find a working compiler” | recipe 里 `CC=gcc -std=gnu17` | 已修复 | 否 |
| 2026-11-09 | tidy | 依赖内部 | tidy-html5 5.8.0 的 `cmake_minimum_required(<3.5)` 被 CMake 4 拒绝 | 加 `-DCMAKE_POLICY_VERSION_MINIMUM=3.5` | 已修复 | 否 |
| 2026-11-09 | librdkafka | 依赖内部 | mklove 的 configure 没有 `--disable-static` / `--disable-tests` | 去掉这两个参数，默认只产共享库 | 已修复 | 否 |
| 2026-11-09 | ImageMagick | 依赖内部 | 自动探测并链接系统 `liblzma` | 加 `--without-lzma`（以及 bzlib/zip/zstd/djvu/lcms 等全部关掉） | 已修复 | 否 |
| 2026-11-09 | imagick | 扩展内部 | 扩展 config.m4 在 GCC 下无条件加 `-lgomp` | `ac_cv_func_omp_pause_resource_all=no` | 已修复 | 否 |
| 2026-11-09 | 依赖目录解析 | 框架 | `enabled_dep_names` 用 `${b%-*}` 拆目录名，`imagemagick-7.1.2-35` 被拆成 `imagemagick-7.1.2` | 改为遍历 recipe 注册表判断目录是否存在 | 已修复 | 是 |
| 2026-11-09 | mongodb | 扩展内部 | 默认 `zstd=auto` 链接系统 `libzstd.so.1`，`verify` 报借用安装根外的库 | `--with-mongodb-zstd=no --with-mongodb-snappy=no` | 已修复 | 否 |
| 2026-11-09 | libpq | 依赖内部 | PostgreSQL 生成 catalog 头的脚本要 `FindBin.pm` 等 perl 核心模块，系统 perl 缺 | `tools=perlshim`，构建前导出 `PERL5LIB` | 已修复 | 否 |
| 2026-11-09 | aom | 依赖内部 | libaom 源码自带 `build/`（上游 cmake 模块），构建目录不能用 `$SRC/build`，清缓存时会把上游模块删掉 | 构建目录改为 `$SRCDIR/aom-build-<版本>` | 已修复 | 否 |
| 2026-11-09 | avif | 依赖内部 | aom 升级换目录后，`$SRC/build/CMakeCache.txt` 还指着 `aom-3.12.0` | cmake 类依赖每次重新配置前 `rm -rf "$SRC/build"`（avif/aom/webp/jpeg/tidy） | 已修复 | 否 |
| 2026-11-09 | 依赖升级 | 框架 | libpng 升级后旧目录被 prune，freetype 的 `.la` 仍记着旧绝对路径，PHP 链接失败 | 依赖安装目录名（含版本）进入 `dep_sig` / `ext_sig` / `php_sig` | 已修复 | 是 |
| 2026-11-09 | jpeg | 框架 | `3.1.90` 是 3.2.0 的预发布编号，被当成补丁更新 | `version_is_prerelease`：最后一段 ≥ 90 视为预发布 | 已修复 | 是 |
| 2026-11-09 | add ext | 框架 | `add ext tidy` 期间 swoole 因签名变化重编，`build.sh` 先删 ini、结束只启用新加的扩展，swoole 变成“编了但没启用” | 构建成功后按 recipe 重写 ini；不再在构建前删 ini（失败时也不会顺带停用） | 已修复 | 是 |

分类：`框架` / `扩展内部` / `依赖内部` / `文档`。

## 10. 优化提案

| 日期 | 方向 | 现状 | 收益 | 成本 / 风险 | 状态 |
| --- | --- | --- | --- | --- | --- |
| 2026-11-09 | `php_sig` 只算有效参数 | `php_sig` 把 `php.sh` 和 `builtin/*.sh` 原文都算进签名 | 改注释不再触发 PHP 重编 | `common.sh` 构建逻辑变化仍靠 `--recompile` | 已实施 |
| 2026-11-09 | `dep_sig` 纳入版本号 | 版本变化靠“新目录不存在”兜底，旧目录残留会误判 | 语义正确，降级/重装不再漏建 | 消费者仍按 SONAME 决定是否重编 | 已实施 |
| 2026-11-09 | 通用 PECL 构建助手 | 7 个 PECL 共 776 行；apcu / igbinary 归一化后只差 18 行 | 两个样本从 170 行降到 53 行 | 需要 ext_prepare 钩子（igbinary 补丁）和自定义 ini | 已实施 |
| 2026-11-09 | `php_sig` 只算选中的 builtin | 原来 `cat` 整个 `builtin/*.sh`，未选中的改动也会触发 PHP 重编 | 加 recipe 不再影响其它项目 | 选中集合由 `SELECTED_EXTS` 决定 | 已实施 |
| 2026-11-09 | `upstream_norm_version` 截断数字结尾包名 | `bzip2-1.0.8` 被解析成 `2` | 修好可少写特例 | 需改公共函数；本轮在 recipe 里剥前缀 | 不采纳（保持现状） |
| 2026-11-09 | 同名 builtin / dep 解析 | `module_sh` deps 优先，`ext_modules libxml2` 取到 dep | 启用 builtin 不再误判 | 新增 `ext_recipe_sh`，只改扩展名解析 | 已实施 |
| 2026-11-09 | 依赖目录进签名 | 消费者只看 SONAME，`.la` 记录绝对路径，prune 后失效 | 依赖升级后消费者会重编，链接不再失败 | 每次 patch 升级多编直接消费者 | 已实施 |
| 2026-11-09 | 预发布版本识别 | PECL 的 `RC1` 被 `<s>stable</s>` 放行，libjpeg-turbo 用 `.90` 表示 beta | 避免推荐不存在的包或测试版 | `version_is_prerelease` + PECL 版本串过滤 | 已实施 |

状态：`待定` / `已批准` / `已实施` / `不采纳`。

## 11. 实测记录

（按批次倒序追加。）

### 2026-11-09 Phase 0 + 签名优化

- wrote `docs/EXTENSIONS.md`、`AGENTS.md` 第 19/20 条。
- 优化 1（`php_sig` 去掉注释和空行）：
  - 只看片段：加注释前后签名都是 `cfdcf7e8b6932ca5`；改一个 `--with-*` 后变成 `af41d1947f636497`。
  - 端到端：给 `builtin/zip.sh` 追加两行注释，`install 8.5.12 --ext "openssl curl zip swoole"`
    输出 `跳过 php（configure 和依赖都没变）`；还原注释后仍跳过。
- 优化 2（`dep_sig` 加版本号）：
  - `dep_sig zlib` 在 `ZLIB_VERSION=1.3.2` 时是 `b66e32b6878ce93c96c9`，
    改成 `1.3.3` 后是 `c155d468884e974ad69a`。
- 签名格式变化后做了一次全量重建（v2demo，2 分 26 秒）；紧接着再跑一次 3.5 秒完成，
  依赖 / PHP / swoole 全部跳过。`phprt verify` 全绿。
- 优化 3（`php_sig` 只算选中的 builtin）：`SELECTED_EXTS` 里属于 builtin 的文件才进签名。

### 2026-11-09 Phase 1a/1b/1c recipe 落地

- 17 个 deps/tools 写成 contract=v2；`scripts/recipe-check.sh --deep` 全绿，
  包括 aom（GCS JSON 列表）、icu（GitHub releases）、sqlite（下载页 7 位编号）。
- `bzip2` / `libxml2` 的 `versions()` 自己剥前缀：`upstream_norm_version` 会把
  `bzip2-1.0.8` 解析成 `2`，公共函数本轮按决定不改。
- 14 个 builtin 写成 v2；`sqlite` / `zlib` 用共享模块 + `20-<名>.ini`。
- `apcu` / `igbinary` 两个 PECL 写成 v2；其余 5 个等“是否加通用助手”的决定。
- 依赖构建（v2ext，17 个库）全部成功：

  | 库 | SONAME | 库 | SONAME |
  | --- | --- | --- | --- |
  | aom 3.12.0 | `libaom.so.3` | libyaml 0.2.5 | `libyaml-0.so.2` |
  | avif 1.3.0 | `libavif.so.16` | oniguruma 6.9.10 | `libonig.so.5` |
  | bzip2 1.0.8 | `libbz2.so.1.0` | openldap 2.6.16 | `liblber.so.2` |
  | freetype 2.13.3 | `libfreetype.so.6` | sqlite 3.54.1 | `libsqlite3.so.0` |
  | icu 79.1 | `libicuuc.so.79` | webp 1.6.0 | `libsharpyuv.so.0` |
  | jpeg 3.1.2 | `libjpeg.so.62` | libffi 3.4.8 | `libffi.so.8` |
  | libpng 1.6.51 | `libpng16.so.16` | libsodium 1.0.22 | `libsodium.so.26` |
  | libxml2 2.15.4 | `libxml2.so.16` | libxslt 1.1.45 | `libxslt.so.1` |
  | re2c 4.6 | 可执行文件（无 .so） | | |

- `apcu` 1 分 28 秒、`igbinary` 26 秒，都是增量构建，没有重编 PHP；
  `php -m` 出现两个模块，功能调用通过，`phprt verify` 全绿。

### 2026-11-09 Phase 1b：7 个 PECL

全部用 `phprt add ext <名字>` 增量安装（不重编 PHP），随后各做一次功能调用：

| 扩展 | 耗时 | 功能调用 |
| --- | --- | --- |
| apcu 5.1.28 | 1 分 28 秒 | `apcu_store` / `apcu_fetch` |
| igbinary 3.2.16 | 26 秒 | `igbinary_serialize` / `unserialize` 往返 |
| msgpack 3.0.1 | 25 秒 | `msgpack_pack` / `msgpack_unpack` |
| protobuf 5.36.2 | 约 30 秒 | `Google\Protobuf\Internal\Message` 可反射 |
| redis 6.3.0 | 约 30 秒 | `new Redis()` 可用 |
| xdebug 3.5.3 | 约 30 秒 | `XDEBUG_MODE=develop` 时 `xdebug_info("mode")` 为 develop |
| yaml 2.3.0 | 约 30 秒 | `yaml_parse` 解析映射和序列 |

- 7 个都通过 `phprt verify`（wheel 完整性）。
- 通用助手落地后，`apcu` / `igbinary` 从 82 / 88 行降到 22 / 31 行，重建后功能不变。

### 2026-11-09 Phase 1c：builtin 全量重编

一次重编 PHP（24 个扩展），编译全部成功。功能调用结果：

| 扩展 | 验证 | 扩展 | 验证 |
| --- | --- | --- | --- |
| bz2 | `bzcompress`/`bzdecompress` | mysqli | `mysqli_get_client_info` |
| ffi | `FFI` 类可反射 | sodium | `sodium_crypto_box_keypair` |
| ftp | `ftp_connect` 存在 | sqlite | `new SQLite3(':memory:')` + `select 1+1` |
| gd | JPEG/WebP/AVIF/FreeType 都是 yes | xsl | `XSLTProcessor` 类存在 |
| gettext | `gettext` 存在 | zlib | `gzcompress`/`gzuncompress` |
| ldap | `ldap_connect` 存在 | zip | `ZipArchive` 类存在 |
| libxml2 | DOM 解析 + XMLReader + SoapClient | mbstring | `mb_strlen("中文")` |
| intl | `NumberFormatter` 格式化 `1,234,567.89` | | |

- 搬移测试：把 `php-8.5.12` 整棵树复制到 `/tmp` 后直接跑 `bin/php`，66 个模块全部加载，
  intl / gd / dom / sqlite / zlib 功能正常。
- `DEFAULT_EXTS` 改成 v1 的 10 个（`openssl zlib libxml2 curl sqlite mysqli mbstring
  zip sodium gettext`），在空项目里跑默认 `install` 验证。

### 2026-11-09 框架修复：同名 builtin / dep 解析

现象：`install` 在启用 `libxml2` 时报“没有编译”，但 PHP 已经编出来了。

原因：`module_sh` 为了不让 `curl` 取错 builtin，解析顺序是 **deps 优先**。于是
`ext_modules libxml2` 读到了 `deps/libxml2.sh`（没有 `modules`），默认成模块名
`libxml2`，而实际模块是 `dom/xml/simplexml/xmlreader/xmlwriter/soap`。
同名的还有 `curl`、`openssl`、`sqlite`、`zlib` 四组。

修复（方案 A）：新增 `ext_recipe_sh`（builtin → ext），`ext_modules` 和
`enable_ext` 改用它；`module_sh` 保持 deps 优先，供 `needs`/`tools` 使用。
PHP 不用重编，重跑 `install` 只补 ini。

### 2026-11-09 Phase 3：SHA256 回填

- 所有 `*_SHA256` 都用实际下载的源码包计算并回填，PHP 除外（原本就是真值）。
- 回填后做了逐项比对：bundle.conf 里的值与缓存包的 sha256 全部一致。
- `fetch` 现在对每一个源码包都会强制校验，不再依赖 TOFU。
- 默认集在空项目（`v2default`）里重新安装验证：6 分 45 秒，48 个 recipe 参与，
  `zlib/libxml2/curl/sqlite/mysqli/mbstring/zip/sodium/gettext` 功能调用全部通过，
  `phprt verify` 全绿。

### 2026-11-09 Phase 2c：amqp / imagick / rdkafka

| 扩展 | 类型 | 依赖 | 验证 |
| --- | --- | --- | --- |
| amqp 2.2.0 | ext | rabbitmq-c 0.18.1（SSL 用树内 openssl） | `AMQPConnection` / `AMQPChannel` 类存在 |
| imagick 3.8.1 | ext | ImageMagick 7.1.2-35 | `newImage` + `setImageFormat("png")` 输出 128 字节 |
| rdkafka 6.0.5 | ext | librdkafka 2.16.0（ssl + zlib） | `RdKafka\Producer` 类存在 |

- ImageMagick 只启用树内已有的格式（png/jpeg/webp/freetype/zlib），其余 delegate 全关；
  不链接系统的 lzma/lcms/tiff/xml 等。
- imagick 关掉 OpenMP（预置 `ac_cv_func_omp_pause_resource_all=no`），不再借用 libgomp。
- 新增 `dep_names`，`enabled_dep_names` 改为按 recipe 名判断，修复带连字符版本
  （`7.1.2-35`）被截断的问题。
- `phprt verify` 全绿。

### 2026-11-09 Phase 2b：pgsql / memcached / mongodb

| 扩展 | 类型 | 依赖 | 验证 |
| --- | --- | --- | --- |
| pgsql + pdo_pgsql | builtin | libpq 18.6（只编客户端） | `pg_connect` / `pg_escape_literal` / PDO pgsql 驱动 |
| memcached 3.4.0 | ext | libmemcached-awesome 1.1.4 | `new Memcached()`；无服务端时 `getStats()` 返回 false |
| mongodb 2.5.7 | ext | 自带 libmongoc/libbson/libmongocrypt | `MongoDB\Driver\Manager` 构造；连接失败抛 `ConnectionTimeoutException` |

- libpq 只构建 `src/interfaces/libpq` + `pg_config`，不编服务端；构建需要 perlshim
  提供 `FindBin.pm`。
- mongodb 关掉 zstd/snappy，避免链接系统库；`verify` 全绿。
- 两个 PECL 的 SHA256 都是真值。

### 2026-11-09 Phase 2a：gmp / tidy / ssh2

| 扩展 | 类型 | 依赖 | 验证 |
| --- | --- | --- | --- |
| gmp 6.3.0 | builtin | gmp 6.3.0 | `gmp_add("12345678901234567890","1")` |
| tidy 5.8.0 | builtin | tidy-html5 5.8.0 | `tidy_parse_string` + `tidy_clean_repair` |
| ssh2 1.5.0 | ext | libssh2 1.11.1 | `ssh2_connect` / `ssh2_auth_password` 存在 |

- 三个新依赖的 SHA256 都是真值。
- 构建过程中修掉三个构建问题（GMP 的 GCC 15、tidy 的 CMake 4、aom 的 `build/` 冲突），
  见问题台账。
- `phprt verify` 全绿；11 个 PECL 在依赖目录变化后按新签名自动重编。

### 2026-11-09 Phase 1.5：升级轮（基线 → 目标）

在 `v2up`（`v2ext` 的整目录副本）里执行，基线树 `v2ext` 保留旧版本可回退。

| 模块 | 基线 | 目标 | 结果 |
| --- | --- | --- | --- |
| libpng | 1.6.51 | 1.6.59 | 重编 1 次；gd 的 PNG/AVIF 仍可用 |
| aom | 3.12.0 | 3.12.1 | 重编 1 次；libavif / gd AVIF 正常 |
| jpeg | 3.1.2 | 3.1.4.1 | 只重编 jpeg，`跳过 php`（SONAME 不变） |
| swoole | 6.2.4 | 6.2.5 | 只重编 swoole，`php -m` 6.2.5 |
| igbinary | 3.2.16 | — | 3.2.17RC1 被过滤，保持 3.2.16 |

- 三个新版本都补了真实 `*_SHA256`（libpng `d80dd2a3…`、aom `9e977518…`、
  jpeg `ecae8008…`、swoole `edafabbf…`）。
- `v2up` 第一次构建时因为复制到新路径、`--prefix` 变化重编了一次 PHP（4 分钟）；
  jpeg 升级在同一路径下完成，PHP 跳过（1 分 25 秒），验证了“同 ABI 补丁不牵连消费者”。
- 升级后 gd（PNG/JPEG/WebP/AVIF/FreeType 全 yes）、swoole、igbinary 功能调用和
  `phprt verify` 都通过。

### 2026-11-09 预发布过滤

- PECL：`igbinary` 的 `3.2.17RC1` 在 PECL 里被标成 stable，且下载地址 404。
  现在 `upstream_versions_pecl` 在归一化前丢弃带 `alpha/beta/rc/dev/pre` 的版本串。
- 通用规则：`version_is_prerelease` 把“最后一段 ≥ 90”的版本视为预发布，
  `version_max` / `version_safe_max` 都经过它；libjpeg-turbo 的 `3.1.90`
  不会再出现在安全更新里。`3.1.4.1` 是真实补丁，正常参与比较。

### 2026-11-09 签名稳定性

- `--ext` 的书写顺序曾影响 `php_sig`：`PHP_DEPS` 和 builtin 文件列表按输入顺序拼接。
  已改成 `sort -u`。实测 `"openssl curl zip ..."` 与 `"igbinary swoole zip curl openssl apcu"`
  两次构建，第二次输出 `跳过 php（configure 和依赖都没变）`。

## 12. 明确不做

imap（uw-imap 维护成本高）、grpc（构建链过重）、mcrypt（已废弃）、
odbc / snmp / pspell / dba / solr / oauth（小众）、readline（`php -a` 非刚需）、
sqlsrv / oci8（私有驱动）。
