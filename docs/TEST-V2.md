# phprtv2 实测记录（PHP 8.5）

测试时间：2026-11-08（第一轮：v2 重构实测；同日稍晚：目录整理后复测；
再后：构建目录按 PHP 版本拆分、精确版不再复制 bundle）。环境：Fedora 43，
源码包复用 `/home/z/testphp/phprt/downloads`（`downloads` 是指向它的软链）。

## 范围

- 只写 PHP 8.5.12。
- 扩展只有 4 个：builtin `openssl`、`curl`、`zip` + ext `swoole`。
- 依赖图：deps `openssl zlib curl libzip`，tools `autoconf perlshim`。
- driver 改造：recipe 即数据（头部 `# phprt:` 行）、driver 即执行器
  （`runner/{common,build,commands}.sh` + `bundles/php8.5/php.sh`）。
- 目录整理：`.phprt/` 顶层只留运行所需；构建期文件按 PHP 版本收进
  `.phprt/build-php-<版本>/`；源码包用 phprt 仓库的 `downloads/`，不复制到项目。
- 精确版安装（`install 8.5.11`，pin 仍为 8.5.12）不复制 bundle：读完仓库
  `bundle.conf` 后由 `apply_php_override` 改 PHP 自身的版本信息，仓库文件不动。

## 目录整理（按版本的构建工作区）

`.phprt/` 顶层只保留运行所需的东西；构建期文件按 PHP 版本分开：

```text
.phprt/
├── current -> php-8.5.12
├── php-8.5.12/            # 标准 PHP 安装树，运行时唯一依赖
├── php-8.5.11/
├── build-php-8.5.12/      # 8.5.12 的构建区；删掉只影响它的增量重建
│   ├── src/               # 解压源码 + 编译对象
│   ├── logs/              # configure / make 日志
│   ├── tools/             # autoconf、perlshim（构建期用）
│   ├── downloads/         # 源码包（仓库 downloads 没有时下载到这里）
│   └── .lock              # flock 构建锁
└── build-php-8.5.11/      # 同上，与 8.5.12 互不干扰
```

目录名和版本树一一对应：`php-8.5.12` 配 `build-php-8.5.12`，一眼能看出是
构建产物、可以删。删掉某一版的构建目录不影响另一版，也不影响运行。

扩展状态不落盘：没有 `project.conf`，也没有每版本的清单文件。需要时按
`myphp` 相同的环境（`PHPRC` / `PHP_INI_SCAN_DIR` / `extension_dir`）跑
`php -m`，再按 recipe 的 `modules` 匹配出已启用的扩展。用户手改 `php.ini`
或删 `.so` 都会反映出来；不同版本各读各的，互不覆盖。

不再生成的东西：`bin/`（runner 副本）、`bundles/`（recipe 副本）、`config`
（仓库绝对路径）、`vendor/`（Perl shim 副本）、`tools/`、`logs/`、
根级 `.phprt.lock`、共享的 `build/` 和 `project.conf`。`phprt` 始终从仓库
运行，recipe 以仓库为准。

源码包查找顺序：`PHPRT_SEED` → phprt 仓库 `downloads/` → 本版本
`build-php-<版本>/downloads/` → 用户缓存 → 旧项目布局（只读兼容）。仓库没有时
才联网下载，写入本版本的 `downloads/`。下载用唯一临时文件 + 原子改名，避免
多项目并发写同一个 `.part`。

实测：

- 删掉 `v2demo/.phprt` 后 `install 8.5 --ext "openssl curl zip swoole" --yes`：
  EXIT=0，`ls -A .phprt` 为 `build-php-8.5.12 current php-8.5.12`；
  `build-php-8.5.12` 约 1.1G，没有 `project.conf`。
- `install 8.5.11 ... --yes`：新建 `build-php-8.5.11/`，`build-php-8.5.12/`
  的文件数（59431）和 inode/时间不变（两版构建互不影响）；两个 `php-8.5.x`
  树并存，`verify` 都通过。
- `install 8.5.11 --plan`：只打印计划，不建任何目录；仓库
  `bundles/php8.5/bundle.conf` 仍是 `PHP_VERSION=8.5.12`。
- 删掉某一版的 `build-php-<版本>` 后 `./myphp -m` 和 `phprt verify` 照常
  （运行不依赖构建区）。
- 删掉构建区 + `swoole.so` + ext 签名后 `phprt add ext swoole`：
  autoconf/perlshim 在该版本的 `build-php-<版本>/tools/` 重建、swoole 从
  仓库 downloads 解压重编、`27-swoole.ini` 写回，EXIT=0。
- 删掉 phprt 仓库 `downloads/` 里的 `curl-8.23.0.tar.xz` 后在空项目重装：
  日志出现 `下载 curl-8.23.0.tar.xz -> .phprt/build-php-8.5.12/downloads/...`，
  其余包仍硬链接仓库种子，EXIT=0。
- `PHPRT_WORK=/tmp/... phprt install 8.5 --plan`：构建目录按
  `<基目录>/build-php-<版本>` 计算，项目 `.phprt` 顶层不变。
- 子目录里 `phprt current` / `phprt verify` 通过 `.phprt/current` 找到项目根。
- 扩展状态取自 `php -m` 的三组实测：
  - 手工删掉 `swoole.so`、保留 `27-swoole.ini`：`phprt show` 只显示
    `curl openssl zip`；`verify` 报 `27-swoole.ini: swoole 没有加载` 和 PHP
    的 `Unable to load dynamic library`，退出码 1。
  - 手工把 `extension=swoole.so` 写进 `etc/php.ini`、移走 `27-swoole.ini`：
    `phprt show` 仍显示 swoole，`verify` 通过。
  - 8.5.11 停用 swoole 后，切到 8.5.12 仍有 swoole；切回 8.5.11 没有 ——
    两个版本的扩展状态互不覆盖。
- v2 项目里残留的 `.phprt/project.conf`：任意 `phprt` 命令会删除并提示；
  v1 项目（`.phprt/bin` 存在，仍在用这个文件）不会被碰。

## 命令与结果

```text
$ phprt install 8.5 --ext "openssl curl zip swoole" --yes
==> 构建 PHP 8.5.12
    扩展: openssl curl zip swoole
PHP 8.5.12
  deps:  perlshim openssl zlib curl libzip autoconf
  php
  ext:   swoole
...
  ok PHP 8.5.12 -> /home/z/testphp/v2demo/.phprt/php-8.5.12
EXIT=0
```

`--plan`（先看计划不构建）：

```text
PHP 8.5.12
  deps:  perlshim openssl zlib curl libzip autoconf
  php
  ext:   swoole
```

依赖顺序是 recipe `needs`/`tools` 拓扑展开的结果：openssl 的 `tools=perlshim`
排在最前，curl/libzip 的 `optional=openssl` 在开关打开时参与。

`phprt verify` 全绿：

```text
==> 1) PHP
    PHP 8.5.12 (cli) (built: Nov 8 2026 17:55:48) (NTS)
==> 2) 已启用的扩展
    ✓ openssl
    ✓ curl
    ✓ zip
    ✓ swoole
==> 3) 动态库（安装根外只允许 glibc 等基础库）
  ok php 与扩展没有从安装根外借库
==> 4) RPATH（只允许 $ORIGIN 相对路径）
  ok RPATH 全部是相对路径
==> 5) 配置路径（树内标准位置）
  ok php.ini 与 conf.d 都在版本目录里
==> 6) CA（只有一份，随项目移动）
  ok HTTPS 请求可用
  ok PHP curl 也走同一个 CA
==> 7) wheel 完整性（sha256 / SONAME / singleton 组 / 无残留原始名）
  ok wheel 完整性通过
```

`./myphp -m` 的模块清单（无 sqlite/dom/libxml 等系统库依赖）：

```text
bcmath calendar Core ctype curl date exif fileinfo filter hash iconv json
lexbor openssl pcntl pcre PDO Phar posix random Reflection session shmop
sockets SPL standard swoole sysvmsg sysvsem sysvshm tokenizer uri
Zend OPcache zip
```

wheel.map（5 个运行时库，OpenSSL singleton 组版本一致）：

```text
libcurl.so.4|libcurl-rt4.so.4|8.23.0|...
libzip.so.5 |libzip-rt5.so.5 |1.12.1|...
libcrypto.so.3|libcrypto-rt3.so.3|3.5.9|...
libssl.so.3 |libssl-rt3.so.3 |3.5.9|...
libz.so.1   |libz-rt1.so.1   |1.3.2|...
```

## 搬移测试

整棵树复制到别的路径后 `./myphp` 与 `phprt verify` 直接可用，无绝对路径残留：

```text
$ cp -a v2demo /tmp/opencode/v2demo-moved && cd /tmp/opencode/v2demo-moved
$ ./myphp -v          # PHP 8.5.12 (cli) ...
$ ./myphp -r '...'    # openssl=1 curl=1 zip=1 swoole=1
$ phprt verify        # 7 步全 ok
```

## 加扩展测试（只加 recipe 文件）

临时加 `bundles/php8.5/ext/redis.sh`（contract=v2 + `build()`/`enable()`）和
bundle.conf 的 `REDIS_*` 三行，然后：

```text
$ phprt add ext redis
  ok redis 6.3.0 -> .../redis.so
  ok 启用扩展 redis（20-redis.ini）
$ ./myphp -m | grep redis
redis
```

driver（`runner/*.sh`、`php.sh`）一行没改。测完删除 recipe 和 bundle.conf 的
pin，`phprt add ext redis` 报 `未知扩展: redis`，回到 4 个扩展的状态。

## 去掉扩展测试

```text
$ phprt remove ext swoole
  ok 停用扩展 swoole        # conf.d/27-swoole.ini 删除
$ ./myphp -m | grep swoole   # 无输出
$ phprt add ext swoole
  ok 启用扩展 swoole（27-swoole.ini）
```

## 幂等性

```text
$ phprt install 8.5 --ext "openssl curl zip swoole" --yes
==> 跳过 php（configure 和依赖都没变）
==> 跳过 ext/swoole（签名未变）
...
  ok PHP 8.5.12 就绪 -> ...
```

依赖/PHP/扩展三层签名都按 recipe 内容 + 解析出的依赖拓扑计算；加不相干的扩展
不会再触发无关依赖重编（签名只含本模块的 `switch` 状态，条件依赖由
`needs()` 的解析结果表达）。

## 在线更新检查（M1）

新增 `phprt outdated`。上游来源写在 recipe 里，公共查询逻辑在 `runner/upstream.sh`：

| 模块 | 来源写法 |
| --- | --- |
| php | `bundle.conf` 的 `PHP_UPSTREAM="phpnet:8.5"` |
| openssl / curl / libzip | `upstream=github:...` |
| swoole | `upstream=pecl:swoole` |
| zlib / autoconf / perlshim | recipe 自己的 `versions()`（目录索引） |

为了验证“判断旧版本”，把 pin 临时改成旧版本再跑（跑完已恢复）：

```text
$ phprt outdated --all
模块       类型   已装       安全更新 最新
php          core     8.5.4        8.5.12       8.5.12
curl         dep      8.22.0       -            8.23.0
libzip       dep      1.12.0       1.12.1       1.12.1
openssl      dep      3.5.4        3.5.9        4.1.0
zlib         dep      1.3.1        1.3.2        1.4
swoole       ext      6.2.3        6.2.5        6.2.5
autoconf     tool     2.71         2.73         2.73
perlshim     tool     5.42.0       5.42.3       5.44.0
```

- curl 8.22.0 → 8.23.0 是跨小版本（8.22→8.23），只出现在“最新”列，不算安全更新；
  zlib 1.3.2 → 1.4 同理。
- **默认范围**：只查当前树 `php -m` 里启用的扩展，再沿 `needs`/`tools` 展开依赖闭包；
  结尾打印「只提示当前 php.ini 配置了的扩展及依赖」。
- **`--all`**：遍历 bundle 里所有已接入模块，不再过滤范围。
- `--json` 输出机器可读结果。
- `OFFLINE=1 phprt outdated` 不联网，打印“离线模式，未查询上游”，退出码 3。
  `--offline` 不是 `outdated` 的参数，它属于 `install` / `update`（M2）。
- 代理：`https_proxy`/`HTTPS_PROXY` 优先，没有时用 `http_proxy`/`HTTP_PROXY`
  （HTTPS 请求也走它）；代理不通会尝试直连，最后才用 `PHPRT_PROXY`。
  实测 `http_proxy=http://127.0.0.1:10808` 可用，坏端口会自动直连兜底。
- 查询结果缓存 6 小时：`~/.cache/phprt/upstream/`。

## recipe 契约门禁（开发用）

只有 `scripts/recipe-check.sh`，不再提供 `phprt recipe` 子命令——它是加 recipe 时的
开发检查，不是用户功能。加扩展/依赖后必须跑通，才算添加成功。

```text
$ scripts/recipe-check.sh
== php8.5 ==
  ok builtin/curl
  ...
  ok tools/perlshim

检查了 10 个 recipe 文件
结果: 通过

$ scripts/recipe-check.sh --deep
  ...
  ok deps/openssl pin 3.5.9
  ok ext/swoole pin 6.2.4
  ok core/php pin 8.5.12
结果: 通过
```

负向验证：

- 临时加一个缺 `upstream=` 的 `ext/badtest.sh`：检查失败，退出码 1，
  并直接打印 `错误: ext/badtest 没有版本来源…` 和文件路径。
- 把 `upstream` 写成 `bogus:x`：提示未知类型，退出码 1。
- 把 `SWOOLE_VERSION` 改成 `9.9.9`：`--deep` 报 `pin 9.9.9 不在上游版本列表`。
- 删除临时文件 / 恢复 pin 后重新通过。

## 扩展迁移与升级轮（2026-11-09）

详细进度、约束和问题台账见 `docs/EXTENSIONS.md`。这里只记结论：

- 38 个 recipe（16 dep + re2c + 14 builtin + 7 PECL）迁到 contract=v2，
  全部构建通过；`scripts/recipe-check.sh --deep` 全绿。
- 7 个 PECL 全部用 `phprt add ext` 增量安装并做功能调用；通用流程
  `runner/recipe-ext.sh` 让每个 recipe 从约 90 行降到 22–31 行。
- 14 个 builtin 一次性重编 PHP，功能调用 14/14 通过；`DEFAULT_EXTS` 改为
  v1 的 10 个，空项目默认安装 6 分 51 秒，`verify` 全绿。
- 同名 builtin/dep（libxml2/sqlite/zlib/curl/openssl）现在分两套解析：
  扩展名用 `ext_recipe_sh`（builtin → ext），依赖用 `module_sh`（deps 优先）。
- 预发布过滤：PECL 的 `RC1` 和 libjpeg-turbo 的 `.90` 不再出现在安全更新里。
- 升级轮（v2up 副本）：libpng 1.6.51→1.6.59、aom 3.12.0→3.12.1、
  jpeg 3.1.2→3.1.4.1、swoole 6.2.4→6.2.5；jpeg 升级只重编自己，
  PHP 跳过（同 ABI），基线树 v2ext 保留旧版本。

## 扩展覆盖最终状态（2026-11-09）

| 类别 | 数量 |
| --- | --- |
| builtin | 20 |
| ext（PECL） | 14 |
| deps | 28 |
| tools | 3 |
| recipe 合计 | 65 |

- 默认集：`openssl zlib libxml2 curl sqlite mysqli mbstring zip sodium gettext`。
- 空项目默认安装、全量项目（30 个扩展）安装都通过；`phprt verify` 全绿。
- 所有 pin 的 `*_SHA256` 已回填并与源码包逐一比对。
- 扩展计划、两轮验证、问题台账见 `docs/EXTENSIONS.md`。

## 过程中修掉的问题

1. PHP configure 默认会开 `sqlite3`/`pdo_sqlite`/`libxml2` 系扩展。phprtv2
   没有这些 recipe，`php.sh` 的基线参数里显式 `--without-sqlite3
   --without-pdo-sqlite --without-libxml --disable-dom --disable-simplexml
   --disable-xml --disable-xmlreader --disable-xmlwriter`；以后加对应 builtin
   recipe，它的 `--flags` 排在后面会覆盖负向参数。
2. `enable_ext` 调 `bash recipe enable` 时 `PHPRT_RUNNER` 未导出，recipe 第 4 行
   `source "${PHPRT_RUNNER:?}/common.sh"` 失败，但函数在 `|| warn` 上下文里被
   吞掉，ini 没写出来却报 ok。修复：`common.sh` 顶部给 `PHPRT_RUNNER` 一个指向
   自己所在目录的兜底值；`enable_ext` 里 `bash "$sh" enable || die`。
3. swoole 的 `--enable-mysqlnd` 在没有 mysqli 的最小 bundle 里会让模块加载报
   `required module "mysqlnd" is not loaded`。phprtv2 不传这个参数。
4. `wheel_integrity_check` 的 OpenSSL 唯一性检查原来写死 `libssl.so.3`，改成读
   recipe 的 `wheel=singleton:libssl+libcrypto`，按「基础名 + .so.」前缀匹配
   wheel.map，组内恰好一份且版本列一致。
5. 签名里扫全部 `ENABLE_*` 会让 `add ext` 时所有依赖/扩展重编；改成只含本模块
   自己的 `switch` 状态。
6. `cmd_add` 的构建锁原来写在 `.phprt/.phprt.lock`，但 `cmd_add` 没有先建
   构建目录，锁文件路径不存在会直接失败；现在锁放在对应版本的
   `build-php-<版本>/.lock`，且 `cmd_add` 先 `ensure_base_files`。
7. 多个项目共享 phprt 仓库 `downloads/` 后，两个项目同时下载同一个包会写同一个
   `.part`；`ensure_source` 改成唯一临时文件 + 原子 `mv`。
8. 精确版安装曾把仓库 `bundles/php8.5/bundle.conf` 用 `sed -i` 改成目标版本，
   污染仓库；改成 `apply_php_override` 在内存里覆盖，仓库文件不动。
9. `build.sh` 的 `REBUILD` 默认值被 `common.sh` 的兼容层提前置 0，导致每次装
   新版本都把依赖源码重新解压；改为在 source `common.sh` 之前设默认值 1。
10. GitHub tags 直接取名字会混入非版本 tag（`brian-gladman-fcrypt-2008-11-18`）
    和预发布（`rc-8_23_0-3`）；现在先过滤 alpha/beta/rc/dev/pre，再用
    `upstream_norm_tag` 只接受「可选的一个字母前缀 + 至少两段版本号」。
11. 目录索引里的链接带 `.tar.gz`，版本提取后残留尾点（`0.8.`），导致版本列表为空；
    `upstream_norm_version` 现在会去掉首尾的点。
12. recipe 头部新增的 `upstream=` 原来没有任何校验：写错类型或包名只会让 `outdated`
    显示 `-`。现在 `recipe_validate` 检查格式，并要求有版本的模块必须声明
    `upstream=`（可为 `none`）或实现 `versions()`；`--deep` 再确认上游能查到 pin。
13. `php_sig` 原来把 `php.sh` 和 `builtin/*.sh` 原文全算进签名，改一行注释就触发
    PHP 全量重编。现在只算去掉行首注释和空行后的内容；`--flags` / `--needs` 的输出
    仍然入签名。实测：加注释后仍是 `跳过 php`，改一个 `--with-*` 签名变化。
14. `dep_sig` 原来不含版本号，版本变化只能靠“新版本目录不存在”兜底；旧目录残留时
    会误判为已完成。现在签名里加了 `version=`。实测 `ZLIB_VERSION` 从 1.3.2 改到
    1.3.3，`dep_sig` 从 `b66e32b6…` 变成 `c155d468…`；消费者是否重编仍由 SONAME 决定。

## 编译方式（static / shared）验收（2026-11-09）

已在 `v2default` 验证：

| 场景 | 命令 | 结果 |
| --- | --- | --- |
| shared builtin（未安装） | `add ext tidy --shared` | 编 `tidy.so`；首次因老树 zlib/sqlite 迁移顺带重编 PHP |
| shared builtin（后续） | `add ext intl --shared`、`add ext ffi --shared` | `跳过 php`，只编 `.so` |
| PECL 默认 shared | `add ext redis --shared` | 编 `redis.so`，`Redis` 可用 |
| PECL static | `add ext redis --static` | 重编 PHP，`php -m` 有 redis |
| PECL static（外部库） | `add ext yaml --static`、`add ext ssh2 --static` | 链接树内 libyaml / libssh2，功能调用通过 |
| static → shared | `add ext redis --shared`、`add ext ssh2 --shared` | 重编 PHP 去掉静态版，再编 `.so` |
| shared → static | `add ext ssh2 --static` | 重编 PHP 编入，删 `.so`/ini |
| 不支持 static | `add ext xdebug --static` | 报错：`type=shared` |
| 契约提示 | `add ext tidy` | 打印 `type` / `default` 和两个参数后退出 |
| install 参数（计划） | `install --build "tidy=static,curl=shared" --plan`、`install --static --plan` | 分组正确；`type=shared` 自动保持 shared 并提示 |

关键约束：

- static PECL 进 PHP 源码树后必须 `buildconf --force`；static 集合变化必须
  `make distclean`，否则旧 `.dep` 会引用已删除的 `ext/<名字>`。
- shared builtin 的 phpize 构建必须带树内 `CPPFLAGS` / `LDFLAGS` / `PKG_CONFIG_PATH`；
  否则 intl 会链接系统 ICU。
- 默认安装把 zlib / sqlite 从 shared 改成 static；空项目默认安装的最终结果
  见下。

### 空项目默认安装（v2mode，2026-11-09）

`rm -rf v2mode && mkdir v2mode && cd v2mode && phprt install 8.5.12`

| 检查 | 结果 |
| --- | --- |
| 构建 | `BUILD-STATUS=ok`；11 个默认扩展全部 static |
| 产物 | `lib/php/extensions/` 不存在（没有任何 `.so`） |
| 模块 | `zlib`、`sqlite3`、`pdo_sqlite`、`Phar` 等都在 `php -m` 里 |
| phar gzip | `Phar::canCompress(Phar::GZ) === true` |
| `phprt show` | `curl(static) … zlib(static)`，无 `?` |
| `phprt verify` | 7 项全绿，wheel 完整性通过 |
| 追装 shared | `add ext gmp --shared` 建出扩展目录，`gmp_add()` 可用 |

顺带修了两个只会在“全 static 树”出现的问题：

- `enable_ext` 假定 `lib/php/extensions/` 一定存在；全 static 安装没有这个目录，
  现在允许缺失，按 `php -m` 判断已编入的模块。
- 启用 ini / 生成 `myphp` 在 `build.sh` 之后，失败时树已经被标成 `ok`，
  下次 `install` 会直接跳过。现在启用前先写 `activating`，成功后才写 `ok`；
  `add ext` 同样处理。
