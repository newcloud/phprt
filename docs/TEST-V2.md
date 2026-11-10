# phprtv2 实测记录（PHP 8.5）

测试时间：2026-11-08（第一轮：v2 重构实测；同日稍晚：目录整理后复测；
再后：构建目录按 PHP 版本拆分、精确版不再复制 bundle）。环境：Fedora 43，
源码包复用 `/home/z/testphp/phprt/downloads`（`downloads` 是指向它的软链）。

## 源码目录调整（2026-11-10）

源码包从 `build-php-<版本>/downloads/` 移到项目级 `.phprt/downloads/`，
`download` 与 `install` / `add ext` / `update` 共用同一套前置下载逻辑。

已验证：

- `phprt download 8.5 --ext "swoole openssl curl" --print-urls` 输出
  `#php` / `#deps-*` / `#tools-*` / `#ext-*` + 实际缓存文件名 + URL，
  且不创建 `.phprt`。
- 不带 `--ext` 时取 `DEFAULT_EXTS`，依赖闭包和层级正确。
- 精确版 `phprt download 8.5.13 --ext "swoole" --print-urls` 的 PHP URL
  换成 8.5.13，依赖仍按 8.5 系列。
- `phprt install 8.5 --ext "gettext" --downloads` 只下载，不创建
  `php-8.5.12/`、`build-php-8.5.12/` 或 `current`。
- 下载完成后再次运行显示“已存在”，不重复下载。
- 下载清单写在 `.phprt/downloads/download-urls-php8.5.12-日期.txt`。
- `install --downloads-print-urls` 只打印，不建目录。
- 构建阶段设置 `PHPRT_NO_DOWNLOAD=1`；`ensure_source` 缺文件直接报错。
- 单个文件超时（`--timeout N`）只中止该文件，保留已下载部分，继续下载其它文件；
  失败文件在最后汇总，退出码非 0。
- 代理只使用用户配置的 `http_proxy` / `https_proxy` / `--proxy`；
  都没配置就直连。
- `--ui progress` 在终端里用 ANSI 自绘的进度区（头部两行 + `─` 分隔线 + 每包一行）；
  非终端自动退回逐行日志。完整 URL 在进度区上方打印一次，进度区不放 URL，避免宽度
  和行数跳动。进度按目标文件大小轮询，支持全局并发 10、单站点并发 2、续传起点、
  速度和超时状态。
- `phprt ext list 8.5` 列出 44 个扩展；`--default` 18 个；`--installed` 在
  v2all 中列出 43 个已启用；`--names`、`--json` 输出正常。
- GitHub 源码先直连，失败后按 `PHPRT_GITHUB_MIRRORS` 顺序换镜像；实测
  tidy 5.8.0 直连超时后由 `gh-proxy.com` 下载成功，SHA256 匹配。
- 装有 `aria2c` 时优先用 aria2c（默认每文件 4 连接）下载，失败自动退回 curl；
  两种方式都验证过本地源下载和 SHA256 校验。
- 代理设置 `HTTP_PROXY` 时也显式传给 `curl -x`，HTTPS 下载会使用它；
  `NO_PROXY` / `no_proxy` 仍然生效。

下面“目录整理”一节记录的是调整前的布局，保留作历史记录。

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
或删 `.so` 都会反映出来；不同版本各读各的，互不覆盖。`phprt show` 在这个
结果上分组显示：静态链接（`php -n -m` 仍列出，说明编进 `bin/php`）、
共享模块（`.so`）、未启用。

`phprt show --tree [扩展]` 会额外打印运行时库的依赖树：根是 `bin/php`
的直接 `NEEDED`，每个扩展下面是它的 `.so`（shared）或第三方依赖入口库
（static）的递归 `NEEDED`。树内库显示成 `libcurl-rt4.so.4` 这样的 rt
SONAME，其余统一归到“外部”。

```text
$ phprt show --tree curl
    链接树:
    PHP 本体（bin/php）:
      ├── libtidy-rt58.so.58
      ├── libssl-rt3.so.3
      ...
      └── 外部: libm.so.6 libgcc_s.so.1 libc.so.6
      curl（静态链接，编入 bin/php）
        └── libcurl-rt4.so.4
            ├── libssl-rt3.so.3
            │   ├── libcrypto-rt3.so.3
            │   │   └── 外部: libc.so.6
            │   └── 外部: libc.so.6
            ├── libz-rt1.so.1
            │   └── 外部: libc.so.6
            └── 外部: libc.so.6
```

全静态树（`--static-all`）里第三方库已经编进 `bin/php`，扩展下面只有
`无树内动态库依赖`，`PHP 本体` 只剩 glibc 等外部库。

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
  （HTTPS 请求也走它）；代理不通会尝试直连。
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
| `phprt show` | `扩展: 已启用 11（静态链接 11，共享模块 0）`，无 `状态未知` |
| `phprt verify` | 7 项全绿，wheel 完整性通过 |
| 追装 shared | `add ext gmp --shared` 建出扩展目录，`gmp_add()` 可用 |

顺带修了两个只会在“全 static 树”出现的问题：

- `enable_ext` 假定 `lib/php/extensions/` 一定存在；全 static 安装没有这个目录，
  现在允许缺失，按 `php -m` 判断已编入的模块。
- 启用 ini / 生成 `myphp` 在 `build.sh` 之后，失败时树已经被标成 `ok`，
  下次 `install` 会直接跳过。现在启用前先写 `activating`，成功后才写 `ok`；
  `add ext` 同样处理。

## `--static-all` 验收（v0，2026-11-09）

目标：扩展和依赖全部链接进 `bin/php`，单独复制可运行；glibc 保持动态。
完整说明见 [STATIC-ALL.md](STATIC-ALL.md)。

最小范围：

```bash
phprt install 8.5 --static-all --ext "openssl zlib curl sqlite mbstring"
```

实际结果（`v2sa`）：

| 检查 | 结果 |
| --- | --- |
| 构建 | `BUILD-STATUS=ok`；5 个扩展都在 `php -m` |
| 链接方式 | `.phprt-state/link` 为 `static`；`phprt show` 显示 `链接方式: static` |
| 共享扩展 | 清理后 `lib/php/extensions/` 不存在 |
| `ldd bin/php` | 只有 `libm.so.6`、`libc.so.6` 和动态加载器 |
| `readelf -d bin/php` | 没有 `deps/`；只有 `$ORIGIN/../lib/phprt` 的相对 RPATH |
| 二进制大小 | 约 38 MB |
| 单文件 | 复制到空目录后 `php -v`、`php -m` 正常 |
| 功能 | `Phar::canCompress(Phar::GZ)` 为 true；PDO sqlite、mbstring、OpenSSL 加密可用 |
| curl | 单文件加系统 CA 后请求 `https://example.com` 返回 200 |
| `phprt verify` | 7 项全绿，新增“静态链接”检查通过 |

排除规则：

```bash
phprt install 8.5 --static-all --ext "tidy xdebug" --plan
```

结果：打印 `xdebug 只支持 shared，--static-all 下已排除（type=shared）`，
static 列表只剩 `tidy`。

切回普通安装：

```bash
phprt install 8.5 --shared --ext "openssl zlib curl sqlite mbstring"
```

结果：`.phprt-state/link` 变回 `shared`，PHP 重新链接到依赖 `.so`，
`phprt verify` 仍全绿；再执行一次 `--static-all` 可以切回单文件。

依赖缺静态库时的报错：

```bash
phprt install 8.5 --static-all --ext "openssl zlib curl sqlite mbstring libxml2"
```

结果：构建在 PHP configure 前停止，报
`依赖 libxml2 没有静态库（--static-all 需要 .a，见 docs/STATIC-ALL.md）`。
这正是 v1 要补的依赖静态库。

实现要点：

- libtool 会把 `-Wl,-Bstatic` / `--start-group` 重排到对象文件之前，不能靠它们
  静态链接。实际做法是建一个只含依赖 `.a` 软链的 `static-libs/` 目录，并放在
  所有 `-L` 前面。
- 静态库是单向扫描，所以 `EXTRA_LIBS` 重复两遍，解决 `libcurl.a` 引用
  `libssl.a` 时的倒序问题。
- configure 自动加的依赖 rpath 必须删掉；`-static-libgcc`/`-static-libstdc++`
  让产物不依赖项目外的 GCC 运行库。

## `--static-all-lto` 验收（v0，2026-11-09）

目标：PHP 本体和依赖都开 LTO，仍然生成单个 `bin/php`。
完整说明见 [STATIC-ALL-LTO.md](STATIC-ALL-LTO.md)。

```bash
phprt install 8.5 --static-all-lto \
  --ext "openssl zlib curl sqlite mbstring"
```

测试树：`/home/z/testphp/v2salto`。

| 检查 | 结果 |
| --- | --- |
| 构建 | `BUILD-STATUS=ok` |
| 链接方式 | `.phprt-state/link` 为 `static` |
| LTO | `.phprt-state/lto` 为 `1` |
| `BUILD-INFO` | `lto_php=1`、`lto_deps=1` |
| 依赖 `.a` | 全部有 `.gnu.lto` 段：openssl 32704+4904、zlib 411、curl 5309、sqlite 3317、oniguruma 2232 |
| `ldd bin/php` | 只有 `libm.so.6`、`libc.so.6` 和动态加载器 |
| `readelf -S bin/php` | 没有 `.gnu.lto` 残留 |
| 单文件复制 | `php -v`、`php -m` 正常 |
| 功能 | sqlite、mbstring、openssl、curl HTTPS 200 正常 |
| `phprt verify` | 全部通过，含新增的 `3c) LTO` |

依赖日志：

- openssl / zlib / oniguruma 的 make 日志能看到 `-flto=auto`。
- curl 走 libtool，make 日志是静默的；以生成的 `Makefile` 和 `.a` 的
  `.gnu.lto` 段为准。
- sqlite 直接调用编译器，不产生独立 make 日志；`.a` 的段检查通过。

切换验证：

```bash
phprt install 8.5 --static-all --ext "..."     # LTO 0，依赖重编
phprt install 8.5 --ext "..."                  # 跳过 php，沿用 LTO 0
phprt install 8.5 --static-all-lto --ext "..." # 切回 LTO 1
```

结果：

- `--static-all` 后 `.phprt-state/lto` 为 `0`，所有依赖 `.a` 的
  `.gnu.lto` 计数变 0，`bin/php` 回到约 38 MB。
- 不带链接参数时显示 `跳过 php（configure 和依赖都没变）`。
- 再执行 `--static-all-lto` 恢复 LTO，`verify` 仍全绿。

### 性能对比（v2sa 非 LTO vs v2salto LTO）

`taskset -c 2`，A/B 交替 5 轮取中位数：

| 用例 | 非 LTO | LTO | 变化 |
| --- | ---: | ---: | ---: |
| `bin/php` 大小 | 38,142,920 | 61,410,416 | +61.0% |
| 启动（200 次，3 轮） | 约 0.78 s | 约 0.78 s | 0% |
| `Zend/bench.php` 总计 | 0.167 s | 0.168 s | +0.6%（噪声内） |
| JSON encode | 98.91 ms | 95.06 ms | -3.89% |
| JSON decode | 253.09 ms | 252.75 ms | -0.13% |
| `mb_strtolower` | 348.63 ms | 337.10 ms | -3.31% |
| `mb_substr` | 30.28 ms | 30.95 ms | +2.21%（噪声内） |
| PCRE `preg_match_all` | 32.95 ms | 33.37 ms | +1.27%（噪声内） |
| zlib `gzcompress` | 187.40 ms | 188.67 ms | +0.68% |
| SHA-256 | 113.96 ms | 113.62 ms | -0.30% |
| OpenSSL AES | 152.70 ms | 152.85 ms | +0.10% |
| SQLite 内存库 | 15.15 ms | 14.51 ms | -4.22% |

### 官方脚本（php-src）

使用 php-src 自带的三个脚本，`taskset -c 2`，A/B 交替 7 轮取中位数：

```bash
SRC=.phprt/build-php-8.5.12/src/php-8.5.12
php -n -d memory_limit=256M "$SRC/Zend/bench.php"
php -n -d memory_limit=256M "$SRC/Zend/micro_bench.php"
php -n -d memory_limit=256M "$SRC/ext/hash/bench.php"

# 另加两组
-d opcache.enable_cli=1 -d opcache.jit=off
-d opcache.enable_cli=1 -d opcache.jit=tracing -d opcache.jit_buffer_size=64M
```

| 脚本 / 选项 | 非 LTO | LTO | 变化 |
| --- | ---: | ---: | ---: |
| `Zend/bench.php` | 171 ms | 171 ms | 0% |
| `Zend/bench.php` + opcache | 110 ms | 110 ms | 0% |
| `Zend/bench.php` + opcache + JIT | 40 ms | 40 ms | 0% |
| `Zend/micro_bench.php` | 745 ms | 736 ms | -1.21% |
| `Zend/micro_bench.php` + opcache | 598 ms | 589 ms | -1.51% |
| `Zend/micro_bench.php` + opcache + JIT | 241 ms | 243 ms | +0.83%（噪声内） |
| `ext/hash/bench.php` 全部用例求和 | 4494 ms | 4489 ms | -0.10% |

说明：

- `Zend/bench.php` 和 `micro_bench.php` 的输出精度是 1 ms；`bench.php`
  总计三组都相同，说明 VM 主路径没有可见变化。
- `micro_bench.php` 的 -1.2% / -1.5% 在 7 轮极差（约 10 ms）内，接近噪声；
  JIT 组反而 +0.8%，不构成稳定收益。
- `ext/hash/bench.php` 里 xxh128 +4.2%、crc32 -4.1%，其它哈希基本不变；
  总计 -0.1%，没有一致趋势。

### json_encode 数据形态复核

v1 文档里 `json_encode` 的 -11.30% 只适用于“1000 条同构记录”的简单形态；
复杂负载只有 -1.6%~-3.5%。这次用 v2sa（非 LTO）和 v2salto（LTO）复核
不同 JSON 形态，`taskset -c 2`、`-n`、A/B 交替 11 轮取中位数：

| 形态 | JSON 大小 | 非 LTO | LTO | 变化 |
| --- | ---: | ---: | ---: | ---: |
| `nested_deep`：1000 条，深层嵌套 | 92 KB | 57.58 ms | 50.93 ms | **-11.55%** |
| `nested`：1000 条，两层嵌套 | 153 KB | 218.84 ms | 200.80 ms | **-8.24%** |
| `records_nested`：1000 条，带 tags/meta | 166 KB | 222.44 ms | 210.64 ms | -5.30% |
| `long_escape`：约 1 MB 转义字符串 | 980 KB | 185.81 ms | 177.51 ms | -4.47% |
| `records_scalar`：1000 条，6 个标量字段 | 99 KB | 167.90 ms | 161.45 ms | -3.84% |
| `records_wide`：1000 条，12 个标量字段 | 220 KB | 82.16 ms | 79.25 ms | -3.54% |
| `records_many_small`：5000 条，3 个字段 | 207 KB | 192.76 ms | 186.49 ms | -3.25% |
| `objects_scalar`：1000 个 stdClass | 99 KB | 204.06 ms | 199.88 ms | -2.05% |
| `array_list`：1000 条纯 list | 55 KB | 197.27 ms | 195.39 ms | -0.95% |
| `long_unicode`：约 3.4 MB Unicode | 3440 KB | 168.35 ms | 168.89 ms | +0.32% |

结论：

- **约 10% 的收益可以复现**，但集中在“记录小、嵌套深、追加次数多”的形态：
  `nested_deep` -11.55%，`nested` -8.24%。
- 普通标量记录约 3%–5%，`stdClass` 和纯 list 约 1%–2%，大 Unicode 字符串没有收益。
- 之前自定义测试的 JSON encode -3.89% 不是矛盾：那个负载是 2000 条带
  `tags`/`meta` 的记录，接近这里的 `records_nested`（-5.30%），不是
  `nested_deep`。
- 所以 v1 的 -11.3% 和这次结果一致：LTO 优化的是 JSON 编码里的
  `smart_str` 追加路径，数据形态越接近“大量小值连续追加”，收益越明显。

### libxml2：XML 解析与 XPath

`libxml2` recipe 现在默认 `--enable-static`，因此 `--static-all` 和
`--static-all-lto` 都能拿到 `libxml2.a`。两棵树都加上 `libxml2` 扩展：

```bash
phprt install 8.5 --static-all \
  --ext "openssl zlib libxml2 curl sqlite mbstring"
phprt install 8.5 --static-all-lto \
  --ext "openssl zlib libxml2 curl sqlite mbstring"
```

- `v2sa`：static 非 LTO，`libxml2.a` 没有 `.gnu.lto` 段。
- `v2salto`：static LTO，`libxml2.a` 有 3865 个 `.gnu.lto` 段。
- `bin/php`：45.9 MB vs 71.5 MB，`ldd` 仍只有 glibc；`phprt verify` 全绿。

负载：2.57 MB XML、12000 个 `<item>`，`taskset -c 2`，`-n`，
A/B 交替 11 轮取中位数：

| 用例 | 非 LTO | LTO | 变化 |
| --- | ---: | ---: | ---: |
| `DOMDocument::loadXML` | 725.01 ms | 709.65 ms | -2.12% |
| `simplexml_load_string` | 698.49 ms | 701.84 ms | +0.48%（噪声内） |
| `XMLReader` 全量遍历 | 227.37 ms | 227.15 ms | -0.10% |
| `xml_parse`（SAX） | 217.80 ms | 215.49 ms | -1.06%（噪声内） |
| XPath `//item[@category='c7']/value` | 418.33 ms | 422.75 ms | +1.06%（噪声内） |
| XPath `//item[name='item-6000']/value` | 432.78 ms | 432.40 ms | -0.09% |
| XPath `count(//item/tags/tag)` | 208.04 ms | 207.68 ms | -0.17% |
| `DOMDocument::saveXML` | 148.05 ms | 142.70 ms | -3.61% |

结论：

- DOM 解析和序列化有约 2%–3.6% 的小幅收益，11 轮里方向比较一致。
- SimpleXML、XMLReader、SAX、XPath 都没有稳定变化；XPath 引擎本身
  没有从这个负载里得到可见加速。
- 收益远小于二进制体积增长，XML 密集型项目可以开，普通项目不必为了
  XML 单独开 LTO。

结论：

- 整体 VM 负载仍然在 ±1% 内，和 v1 只开 PHP LTO 的结论一致。
- 依赖 LTO 后，JSON 编码、mbstring、SQLite 出现 3%–4% 的局部变化；
  zlib 和 OpenSSL 在这组用例里没有明显收益。
- 二进制从 38 MB 增到 61 MB，启动时间不变；是否值得取决于项目是否
  主要跑 JSON、mbstring、SQLite 这类路径。

## buildroot / chroot（2026-11-09）

standalone 和自动 `--chroot` 已接入。真实环境用 sudo 跑通了 `create` 和
`install --chroot`；用户命名空间（`unshare -rmp`）另外验证挂载、bind、
rootfs 可见性和清理逻辑：

| 检查 | 结果 |
| --- | --- |
| `phprt buildroot help/status/list/env` | 正常；不存在的 rootfs 直接报“不存在” |
| `create`（真实 dnf） | 409 包 / 659M；`.phprt-buildroot`、`.config`、默认路径正确 |
| `create`（fake dnf / 用户命名空间） | 写出 rootfs、标记、配置；`list` 标默认；`remove` 后清空 |
| `install --chroot` | `openssl zlib curl sqlite mbstring` 在 chroot 内构建成功 |
| `add ext --chroot` | `tidy --shared` 构建成功，功能调用通过 |
| 宿主 verify | `phprt verify` 7 项全绿 |
| `--host` / `--chroot` | 配置在 host/chroot 间切换，不触发无意义重编 |
| `create` 目标安全 | 拒绝非空非 rootfs 目录；拒绝仓库目录 |
| `remove` 安全 | 缺 `.phprt-buildroot` 标记时拒绝删除 |
| `exec` 挂载 | `/proc`、`/dev`、项目、仓库、home 均挂载成功；用户命名空间下 `sysfs` 失败只警告 |
| rootfs 在项目内 | chroot 内可再看到一次 rootfs；构建只用精确路径，不递归扫描 |
| 项目 / 仓库可见 | `pwd`、`PHPRT_PROJECT`、`PHPRT_REPO` 路径保持不变 |
| HOME | `/home/phprt` 指向 `<项目>/.phprt/buildroots/.home` |
| 退出清理 | `exec` / `enter` 结束后 `findmnt` 中 rootfs 下挂载数为 0 |
| 产物属主 | `bin/php`、依赖 `.so`、测试文件均为宿主 `1000:1000` |
| `run_build_env` | 在 chroot 内执行命令，`HOME`、`BUILD_ROOT`、项目路径正确 |
| `remove`（真实 rootfs） | 先卸载再删除，默认路径重置，无残留挂载 |
| `create` 用户条目 | rootfs 的 `/etc/passwd` / `/etc/group` 加入宿主用户，`id` 正常 |
| 中断清理 | `exec -- sleep` / `add ext gd --chroot` 挂载后发 `SIGTERM`：终止 chroot 进程树，挂载数回到 0 |
| 失败清理 | curl 下载失败导致构建退出：挂载数同样回到 0 |

## `phprt update`（2026-11-09）

依赖库的同系列安全更新：

| 检查 | 结果 |
| --- | --- |
| `phprt update openssl` | 查到最新安全版本等于当前 3.5.9，提示“已是最新安全版本” |
| `phprt update openssl 3.5.10 --dry-run` | 打印当前、目标、URL、缓存状态，不下载不构建 |
| `phprt update openssl 3.5.8` | 拒绝降级 |
| `phprt update openssl 4.1.0` | 拒绝跨系列 |
| `phprt update nope` | 报未知依赖 |
| `OFFLINE=1 phprt update openssl` | 明确提示不能查上游，要求指定版本 |
| `OFFLINE=1 ... 3.5.10 --dry-run` | 正常打印计划 |

测试时上游没有比 3.5.9 更高的 3.5.x，所以先用 dry-run 和错误路径验证参数
处理；再在副本仓库里把 OpenSSL 降到 3.5.8，实际跑 `phprt update openssl`：

- 自动选中 3.5.9，复用已缓存源码，重编 OpenSSL；
- PHP 签名不变时跳过 PHP，wheel 重新整理后 `verify` 全绿；
- 用损坏的 3.5.10 源码包触发构建失败，确认回滚恢复 `bundle.conf`、旧依赖
  目录和 `lib/phprt`，`myphp` 仍能启动。

## 扩展交互选择与 gd 子选项（2026-11-10）

| 检查 | 结果 |
| --- | --- |
| `bash gd.sh --opts` | 输出 5 行 `key<TAB>state<TAB>label`；jpeg/webp/freetype=on，png=fixed，avif=off |
| 默认 flags / needs | `--with-jpeg --with-freetype --with-webp --without-avif`；deps 为 libpng jpeg freetype webp，无 aom/avif |
| `--opt gd=jpeg,png,webp,avif,freetype` | 增加 `--with-avif`，deps 增加 aom avif |
| `--opt gd=jpeg,png` | 只保留 jpeg；freetype/webp/avif 都输出 `--without-*` |
| `--opt gd=` | 只保留固定的 png（libpng） |
| `--opt gd=bogus` | 报错并列出可用子项 |
| `install --ext gd --print-urls` | 默认不出现 aom/avif；显式开 avif 后出现 |
| `phprt select 8.5`（pty） | 打开时停在最后一行「确认并继续」；直接回车使用默认 18 个扩展 |
| 主列表 gd（pty） | 收起时 `[ ] gd ▶`；展开后子项为 `    [ ] jpeg` / `    [x] png (固定)` / … |
| 三态标记（pty） | 未选中 `[ ]`；选中但 avif 关时 `[-]`；五个子项全开时 `[x]`；无子项扩展选中直接 `[x]` |
| 展开/收起（pty） | gd 上 Enter 显示 `▼` 和 5 个子项，再按 Enter 回到 `▶`；普通扩展上 Enter 无动作 |
| Space 切换（pty） | 父行切换该扩展选中；子行切换子项，父扩展未选中时会先被选中 |
| 固定子项（pty） | gd 未选中时 png 仍显示 `[x] png (固定)`；灰色显示，Space 无效 |
| 子项记忆（pty） | 关闭 gd 再打开，子项配置保持（如 jpeg 开、webp 关） |
| Esc（pty） | 子项或已展开的父行上 Esc 先收起、光标回到父行；收起状态下再按 Esc 退出码 130 |
| 按键延迟（pty） | 单帧 5–20ms（列表和 opts() 元数据已缓存），帧间无清屏 |
| 屏幕管理（pty） | 默认同屏绘制，全程没有 smcup/rmcup；每帧以 `\033[0J` 收尾，无 `\033[2J` 重绘 |
| 命令行保留（pty） | 先打印 `$ phprt select 8.5` 再启动；菜单出现在下一行，运行中和退出后该行都在 |
| 选中行样式（pty） | 光标行输出 `\033[1;97;44m`（加粗 + 蓝底）；`NO_COLOR=1` 时用 `\033[1;7m` 反显 |
| alternate screen（pty） | `PHPRT_ALT_SCREEN=1` 时各 1 次 smcup/rmcup，退出恢复原屏幕 |
| Home/End（pty） | 打开在确认行，`Home` 跳到第一个扩展，`End` 回确认行 |
| 小窗口（pty，LINES=10） | 视口 6 行（同屏模式预留 4 行），头尾固定，可滚动 |
| 确认方式 | 只有主菜单最后一行，回车完成；直接回车使用默认 18 个扩展 |
| 非终端 / `--yes` | 不进入菜单，使用默认集 |
| `scripts/recipe-check.sh` | 65 个 recipe 通过，新增 opts() 格式与变量检查 |
| 实际构建 gd（默认子项） | `/tmp` 空项目 `install 8.5 --ext gd --yes` 成功；`gd_info` 为 JPEG/PNG/WebP/FreeType=1、AVIF 空；`.build-opts` 和 `BUILD-PLAN` 记录 `gd=jpeg,png,webp,freetype` |
| `phprt select` 读取已装树 | 在 gdbuild 中预选 gd，并读回 `.build-opts` 的 jpeg/png/webp/freetype |
| v2all `phprt verify` | 7 项全绿 |

## 新增子项：pgsql/curl/zip/redis/openssl（2026-11-10）

| 检查 | 结果 |
| --- | --- |
| `bash <recipe> --opts` | pgsql `ssl(on)`；curl `sftp(off)`；zip `bzip2(off)`；redis `lzf(off)`；openssl `legacy_provider(off)` |
| `scripts/recipe-check.sh` | 65 个 recipe 通过；5 个新 key 都在 recipe 里被使用 |
| `install --plan`（六个扩展，on 状态） | deps 为 `perlshim openssl zlib libssh2 curl bzip2 libzip libpq autoconf` |
| `add ext redis --shared --opt redis=lzf` | `Redis::COMPRESSION_LZF` 为 true；未重编 PHP |
| `add ext pgsql --static --opt "pgsql=ssl curl=sftp zip=bzip2 openssl=legacy_provider"` | 一次 PHP 重编全部生效；`pg_config --configure` 为 `--with-ssl=openssl --without-zlib --without-icu`，libpq 链接树内 libssl/libcrypto |
| curl sftp | `curl_version()["protocols"]` 包含 `sftp`；php 的 NEEDED 有 `libssh2-rt1.so.1` |
| zip bzip2 | 写入 `ZipArchive::CM_BZIP2` 后 `comp_method=12`，读回内容一致；php 的 NEEDED 有 `libbz2-rt1.0.so.1.0` |
| openssl legacy_provider | `openssl_get_cipher_methods()` 含 `des-ede3-cbc`、`rc4`、`bf-cbc` |
| `add ext --opt` 合并 | 树上已有 6 项子选项时，只传 `redis=lzf`，`.build-opts` 其余 5 项保持不变 |
| `add ext` 无 `--opt` | 已安装且模式相同：1.5 秒返回，不重编 |
| v2ext `phprt verify` | 7 项全绿 |
| `install` 全量（`/tmp` 新项目，带 5 项 `--opt`） | 构建成功；`.build-opts` 与 `BUILD-PLAN` 的 `EXT_OPTS=` 都是 `openssl=legacy_provider curl=sftp zip=bzip2 pgsql=ssl redis=lzf` |
| install 树功能调用 | `sftp=yes`；zip `method=12 ok=yes`；`Redis::COMPRESSION_LZF` true；legacy 算法可用；libpq 链接 libssl/libcrypto |
| install 树 `phprt verify` | 7 项全绿；`show --tree pgsql` 显示 libpq → libssl → libcrypto |

> 实测发现：PG 18 的 libpq 只用 `--with-ssl=openssl`；zlib/ICU 是服务端用的，
> libpq 产物没有 libz/ICU 依赖，所以 pgsql 只有 `ssl` 一个子项。

## 数据库扩展拆分与分类菜单（2026-11-10）

| 检查 | 结果 |
| --- | --- |
| recipe 结构 | `mysqli` / `pdo_mysql`、`pgsql` / `pdo_pgsql` 各自独立；`sqlite3` 一个 recipe 同时产出 `sqlite3` + `pdo_sqlite` |
| 扩展总数 | `phprt ext list 8.5` 列出 44 个（builtin 30 + PECL 14） |
| `--plan --ext mysqli` / `pdo_mysql` | deps 只有 `zlib`（mysqlnd 压缩协议）；不出现 libpq/sqlite |
| `--plan --ext pgsql` / `pdo_pgsql` | deps `perlshim openssl libpq` |
| `--plan --ext "pgsql pdo_pgsql"` | libpq 只出现一次 |
| `--plan --ext sqlite3` | deps 只有 `sqlite` |
| 默认集 | `openssl zlib libxml2 curl sqlite3 mysqli pdo_mysql mbstring zip sodium gettext` + 7 个 sys/pcntl 扩展（共 18 个） |
| `add ext pdo_mysql --static`（v2demo） | `PDO` + `pdo_mysql`，没有 `mysqli`；`--with-pdo-mysql=mysqlnd --without-mysqli` |
| `add ext sqlite3 --static` | 同时出现 `sqlite3` 与 `pdo_sqlite`，PDO 驱动新增 sqlite |
| `add ext pdo_pgsql --static` | `pdo_pgsql` 可用；`pg_config --configure` 含 `--with-ssl=openssl`，libpq 链树内 libssl/libcrypto |
| 老树识别 | v2ext 的 `phprt show` 自动列出 `pdo_mysql pdo_pgsql sqlite3`（按模块名反推） |
| 菜单（pty） | 分组顺序 Database / Cache & Storage / Compression / …；`Home` 落在 `mysqli` 且上方 `Database` 标题可见，`End` 回确认行，标题行不可选中 |
| 菜单层级（pty） | 分类标题顶格、`\033[1;36m`（NO_COLOR 时 `\033[1m`）；扩展行缩进 2、子项缩进 6；分类之间、确认行上下各一个空行（条目 54 → 63，帧多一行） |
| 分类内顺序 | Database 为 mysqli、pdo_mysql、pdo_pgsql、pgsql、sqlite3、mongodb（`menu_order`） |
| `--ext sqlite` | 报错：`未知扩展: sqlite；SQLite 已合并到 sqlite3（同时产出 pdo_sqlite）` |
| `scripts/recipe-check.sh` | 67 个 recipe 通过（含 category 必填校验） |
| 全量 `install`（仅数据库扩展） | `/tmp` 新项目 `--ext "openssl mysqli pdo_mysql pgsql pdo_pgsql sqlite3"` 成功；`PDO::getAvailableDrivers()` 为 mysql/pgsql/sqlite |
| install 树 `phprt verify` | 7 项全绿；mysqlnd 压缩协议链的是树内 `libz-rt1.so.1` |
| 框架修复 | `enabled_dep_names` 在最后一个依赖未构建时返回 1，导致 `php.sh` 在 `set -e` 下静默退出；已加 `return 0` |

## 新增 builtin：pcntl / posix / readline / sockets / shmop / sysv*（2026-11-10）

原来 7 个扩展写死在 `bundles/php8.5/php.sh` 的 configure 参数里（选择器看不到、
也关不掉），readline 完全没编。本轮改成 builtin recipe；readline 另外新增
ncurses 6.5 + GNU readline 8.3 两个依赖。

| 检查 | 结果 |
| --- | --- |
| `phprt ext list 8.5 --names` | 44 个（新增 8 个） |
| `phprt ext list 8.5 --default --names` | 18 个（新增 7 个，readline 默认关） |
| `install --yes --plan`（空项目） | static 18 个；deps 为 `perlshim openssl zlib libxml2 curl sqlite oniguruma libzip libsodium`，不含 ncurses/readline |
| `install --ext "sockets"`（空项目） | `php -m` 只有 sockets；pcntl / posix / shmop / sysv* 都不出现（posix 在 PHP 里默认启用，靠 recipe 的 `--disable-posix` 关掉） |
| `install --ext "<8 个>"` | 一次 PHP 重编，8 个模块都进 `php -m` |
| 功能调用 | `pcntl_fork` + `pcntl_waitpid`（子进程 rc=7）、`posix_getpid`、`socket_create`、`shmop_open/write/read`、`msg_get_queue/msg_send/msg_receive`、`sem_get/acquire/release`、`shm_attach/put_var/get_var`、`readline_add_history/readline_list_history` |
| readline 依赖 | `install --ext readline --plan` 的 deps 为 `ncurses readline`；`bin/php` 的 NEEDED 有 `libreadline-rt8.so.8` / `libncurses-rt6.so.6`（wheel 改名 + `$ORIGIN` rpath） |
| `--build sockets=shared` | 静态 sockets 被清掉，生成 `sockets.so` + `20-sockets.ini`，`socket_create()` 正常，`verify` 全绿 |
| `--build readline=shared` | 报错 `readline 不支持 shared（type=static）`：ext/readline 用 `PHP_ARG_WITH`，phpize 会把 libedit 强制成 shared |
| 搬移测试 | 整棵树 `cp -a` 到别的目录后，`bin/php -m` 和功能调用都正常 |
| 选择器（pty） | Runtime 分组出现 pcntl/posix/readline，Network 出现 sockets，Other 出现 shmop/sysvmsg/sysvsem/sysvshm；都没有展开箭头；Space 能开关 readline 并反映到输出 |
| `scripts/recipe-check.sh` | 77 个 recipe 通过 |
| `phprt verify` | 7 项全绿（含 shared sockets） |

ncurses 6.5 / readline 8.3 的三个坑（recipe 里已固定）：

- ncurses 默认开 widec，只产出 `libncursesw.so`；PHP 的 ext/readline 用
  `AC_CHECK_LIB([ncurses], [tgetent])` 只认 `libncurses`，所以加 `--disable-widec`。
- ncurses 的 C++ 绑定在 GCC 15 下编不过（`std::hash<unsigned char>` 重定义），
  必须 `--without-cxx --without-cxx-binding`。
- readline 的共享库默认不链 termcap（`SHLIB_LIBS` 为空），`UP` / `BC` / `tgetent`
  留给调用方；加 `--with-shared-termcap-library` 才会把 `libncurses` 写进 NEEDED。
  不加的话，编扩展时 `LD_LIBRARY_PATH` 里的 libreadline 会让宿主的 `awk`（gawk）
  加载失败（`undefined symbol: UP`）。

## 下载进度 ANSI 重写（2026-11-10）

| 检查 | 结果 |
| --- | --- |
| 去掉 gum | `source_ui_progress` 不再检查 `gum`；`source_progress_render_frame` 删除，系统没装 gum 也能画进度 |
| 帧结构 | 头部两行 + `─` 分隔线 + 每包一行；无边框、无整屏 `\033[2J` |
| 帧重绘 | 整帧一次写出；先上移上一帧行数，逐行 `\r\033[2K`，末尾 `\033[0J`；内容不变不重画 |
| 缩短场景（pty） | 先画 5 行再画 2 行，最终只剩 `alpha`/`beta`，没有 `three`/`five` 残留 |
| 50 列终端（pty） | 走 compact 行，标签截到 18 字符，行宽不超过 50，没有换行错位 |
| `PHPRT_UI=auto` 非终端 | 返回 1，走逐行日志 |
| `PHPRT_UI=progress` 非终端 | 提示“输出不是终端，改用逐行下载日志”并返回 1 |
| `PHPRT_UI=plain` | 一直返回 1，走逐行日志 |
| 桩下载（pty） | 3 个包并发下载，最终帧显示分隔线和 `✓ 完成`，退出码 0 |

| `install --downloads`（pty） | 不写 `--ext` 时先进入 ANSI 选择界面；Esc 退出码 130，不创建 `.phprt`；确认后只下载，不构建 |
