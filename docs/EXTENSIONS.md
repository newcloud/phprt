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
| 6 | **目录**：dep → `<tree>/deps/<名>-<版本>`；tool → `.phprt/build-php-<版本>/tools/`；builtin / ext → PHP 树。`.phprt` 顶层只允许 `current`、`php-*`、`build-php-*` | `AGENTS.md` 17 |
| 7 | **没有 `SUPPORTED_EXTS`**：扩展清单就是 `{builtin,ext}/*.sh` 的文件名 | `common.sh:builtin_names` |
| 8 | **扩展状态不落盘**：启用状态由 `php -m` 推导，不写 `project.conf`；用户删 ini 或 `.so` 后结果仍要准 | `AGENTS.md` 16 |
| 9 | **ini 必须显式**：builtin 静态写 `ini=-`，共享模块写 `20-<名>.ini`；ext 必须实现 `enable()` | `RECIPE.md` 3.1 |
| 10 | **wheel 化不手写 ELF**：库放 `lib/` 或 `lib64/` 根目录，真实文件是 `.so.N` 且 SONAME 带 ABI 号；例外只用 `wheel=none/exclude/alias/singleton` | `RECIPE.md` 6 |
| 11 | **不写系统目录、不引入第二份 CA**；CA 固定在 `etc/ssl/cert.pem` | `AGENTS.md` 9/10 |
| 12 | **重编代价**：改 builtin 会改 PHP 签名 → PHP 全量重编；dep / ext 只重编自己 | `build.sh` |
| 13 | **下载/离线**：源码包优先仓库 `downloads/`（当前软链到 v1 的），缺的进 `build-php-<版本>/downloads/`；`OFFLINE=1` 是全局离线开关 | `common.sh:find_source` |
| 14 | **尚未实现**：`phprt update`、install / add 的在线检查（M2）、`--no-update-check`、`--offline` 参数；多数 `*_SHA256` 仍为空（TOFU） | 代码现状 |
| 15 | **v1 / v2 不能共用一个 `.phprt`**，会互相覆盖。测试项目分开：`v2demo` 最小集、`v2ext` 扩展开发集 | 实测 |
| 16 | **暂缓/不做**：buildroot / chroot；imap、grpc、mcrypt、odbc、snmp、pspell、dba、solr、oauth、readline、sqlsrv、oci8 | v1 文档 |

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

| 名称 | 类型 | 依赖 | 版本变量 | 基线 | 目标 | 升级增量 | 状态 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| re2c | tool | — | `RE2C_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| aom | dep | — | `AOM_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| avif | dep | aom | `AVIF_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| bzip2 | dep | — | `BZIP2_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| freetype | dep | zlib, libpng | `FREETYPE_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| icu | dep | — | `ICU_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| jpeg | dep | — | `JPEG_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| libffi | dep | — | `LIBFFI_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| libpng | dep | zlib | `LIBPNG_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| libsodium | dep | — | `LIBSODIUM_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| libxml2 | dep | zlib | `LIBXML2_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| libxslt | dep | libxml2 | `LIBXSLT_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| libyaml | dep | — | `LIBYAML_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| oniguruma | dep | — | `ONIGURUMA_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| openldap | dep | — | `OPENLDAP_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| sqlite | dep | — | `SQLITE_VERSION` | 待定 | 待定 | 待定 | ⬜ |
| webp | dep | — | `WEBP_VERSION` | 待定 | 待定 | 待定 | ⬜ |

### 4.2 PECL 扩展（7）

| 名称 | 依赖 | 版本变量 | ini | 基线 | 目标 | 升级增量 | 状态 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| apcu | — | `APCU_VERSION` | `25-apcu.ini` | 5.1.28 | 待定 | 待定 | ⬜ |
| igbinary | — | `IGBINARY_VERSION` | `20-igbinary.ini`（待确认） | 3.2.16 | 待定 | 待定 | ⬜ |
| msgpack | — | `MSGPACK_VERSION` | `21-msgpack.ini`（待确认） | 3.0.1 | 待定 | 待定 | ⬜ |
| protobuf | — | `PROTOBUF_VERSION` | `23-protobuf.ini` | 5.36.2 | 待定 | 待定 | ⬜ |
| redis | — | `REDIS_VERSION` | `20-redis.ini` | 6.3.0 | 待定 | 待定 | ⬜ |
| xdebug | zlib | `XDEBUG_VERSION` | `30-xdebug.ini` | 3.5.3 | 待定 | 待定 | ⬜ |
| yaml | libyaml | `YAML_EXT_VERSION` | `26-yaml.ini` | 2.3.0 | 待定 | 待定 | ⬜ |

> igbinary / msgpack 在 v1 里写 ini 的位置需要迁移时核对（v1 审计表没有列出），
> 以 v1 recipe 的实际文件名为准，并同步更新本表。

### 4.3 builtin（14）

| 名称 | 依赖 | modules | ini | 升级增量 | 状态 |
| --- | --- | --- | --- | --- | --- |
| bz2 | bzip2 | bz2 | - | 无（builtin） | ⬜ |
| ffi | libffi | ffi | - | 无（builtin） | ⬜ |
| ftp | — | ftp | - | 无（builtin） | ⬜ |
| gd | zlib, libpng, jpeg, freetype, webp, aom, avif | gd | - | 无（builtin） | ⬜ |
| gettext | —（glibc） | gettext | - | 无（builtin） | ⬜ |
| intl | icu | intl | - | 无（builtin） | ⬜ |
| ldap | openldap | ldap | - | 无（builtin） | ⬜ |
| libxml2 | libxml2 | dom,xml,simplexml,xmlreader,xmlwriter,soap | - | 无（builtin） | ⬜ |
| mbstring | oniguruma | mbstring | - | 无（builtin） | ⬜ |
| mysqli | —（mysqlnd） | mysqli,pdo_mysql | - | 无（builtin） | ⬜ |
| sodium | libsodium | sodium | - | 无（builtin） | ⬜ |
| sqlite | sqlite | sqlite3,pdo_sqlite | `20-sqlite.ini` | 无（builtin） | ⬜ |
| xsl | libxslt | xsl | - | 无（builtin） | ⬜ |
| zlib | zlib | zlib | `20-zlib.ini` | 无（builtin） | ⬜ |

> builtin 的版本跟 PHP 走，没有独立的“降级/升级”。等价验证是：关闭时 `php -m` 里没有，
> 开启并重编后有；PHP 自身升级单独做一轮（8.5.11 → 8.5.12）。

## 5. 新增清单（9 项，v1 计划表剩余）

| 扩展 | 类型 | 新依赖 | PHP 8.5 支持 | configure 参数 | 基线 | 目标 | 升级增量 | 状态 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| gmp | builtin | gmp | 待查 | 待查 | — | — | 无（builtin） | ⬜ |
| tidy | builtin | tidy-html5 | 待查 | 待查 | — | — | 无（builtin） | ⬜ |
| ssh2 | ext | libssh2 | 待查 | 待查 | 待定 | 待定 | 待定 | ⬜ |
| pgsql + pdo_pgsql | builtin | libpq | 待查 | 待查 | — | — | 无（builtin） | ⬜ |
| memcached | ext | libmemcached-awesome | 待查 | 待查 | 待定 | 待定 | 待定 | ⬜ |
| mongodb | ext | mongo-c-driver（libbson + libmongoc） | 待查 | 待查 | 待定 | 待定 | 待定 | ⬜ |
| amqp | ext | rabbitmq-c | 待查 | 待查 | 待定 | 待定 | 待定 | ⬜ |
| imagick | ext | ImageMagick 7 | 待查 | 待查 | 待定 | 待定 | 待定 | ⬜ |
| rdkafka | ext | librdkafka | 待查 | 待查 | 待定 | 待定 | 待定 | ⬜ |

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

- builtin：`flags()` / `needs()`，`modules` 写全，`ini` 显式。
- ext：`build()` + `enable()`。
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
| — | — | — | （开工后填写） | — | — | — |

分类：`框架` / `扩展内部` / `依赖内部` / `文档`。

## 10. 优化提案

| 日期 | 方向 | 现状 | 收益 | 成本 / 风险 | 状态 |
| --- | --- | --- | --- | --- | --- |
| 2026-11-09 | `php_sig` 只算有效参数 | `php_sig` 把 `php.sh` 和 `builtin/*.sh` 原文都算进签名 | 改注释不再触发 PHP 重编 | `common.sh` 构建逻辑变化仍靠 `--recompile` | 已实施 |
| 2026-11-09 | `dep_sig` 纳入版本号 | 版本变化靠“新目录不存在”兜底，旧目录残留会误判 | 语义正确，降级/重装不再漏建 | 消费者仍按 SONAME 决定是否重编 | 已实施 |
| 2026-11-09 | 通用 PECL 构建助手 | 每个 ext 复制约 90 行 phpize 流程 | 15 个扩展省约 1300 行重复 | 等重复实际出现再评估 | 待触发 |

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

## 12. 明确不做

imap（uw-imap 维护成本高）、grpc（构建链过重）、mcrypt（已废弃）、
odbc / snmp / pspell / dba / solr / oauth（小众）、readline（`php -a` 非刚需）、
sqlsrv / oci8（私有驱动）、buildroot / chroot（暂缓）。
