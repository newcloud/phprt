# phprt

**免 root、可离线、可沙箱的 PHP 工具链。** 从源码编一套 PHP，装进一个目录
（`~/.phprt` 或项目内的 `./.phprt`），项目里只保留一个自包含的 `./myphp`。

```bash
./myphp -v                      # PHP 8.5.12 / 8.6.0-dev，随项目走
./myphp worker.php              # 跑常驻 worker（RoadRunner / Swoole）
./myphp --sandbox worker.php    # 同一个命令，跑进 bwrap 沙箱
```

不写 `/usr/local`、不改 shell 配置、不需要 root；卸载就是 `rm -rf` 那个目录。

---

## 目录

- [为什么不用系统里的 PHP](#为什么不用系统里的-php)
- [特性](#特性)
- [快速开始](#快速开始)
- [项目里的 `./myphp`](#项目里的-myphp)
- [沙箱（bwrap）](#沙箱bwrap)
  - [完整例子：RoadRunner 博客](#完整例子roadrunner-博客)
- [多版本](#多版本)
- [扩展](#扩展)
- [命令参考](#命令参考)
- [安装根布局](#安装根布局)
- [常见问题](#常见问题)
- [文档](#文档)

## 为什么不用系统里的 PHP

系统包管理器装的 PHP 是「系统的 PHP」：它链接系统里的 libcurl / libssl / libicu，
系统一升级，扩展行为可能跟着变；通常只能有一个默认版本；换版本要动系统仓库。

phprt 装的是「项目的 PHP」：

| | `dnf/apt install php` | phprt |
| --- | --- | --- |
| 依赖来源 | 全部来自系统 | 只有 glibc（`libc/libm/libgcc_s`），其余都在 `deps/` |
| 系统升级影响 | 换 libcurl/libssl 就可能变行为 | 无（除非 glibc ABI 变化） |
| 多版本 | 一般只能一个默认版本 | 8.5 / 8.6 并存，项目级切换 |
| 回滚 | 动系统仓库 | 改 `./myphp` 一行 + 重启进程 |
| 隔离 | 能读 `$HOME`、`/etc/shadow`、别的项目 | 可选 bwrap 沙箱，只留 glibc 白名单 |
| 卸载 | 卸载包 | `rm -rf ~/.phprt` |
| 代价 | 随发行版更新 | 自己编（约 20 分钟）、每版 100–200MB |

可审计性也是卖点：`ldd` 出来的外部依赖就 4 行，`phprt verify` 会逐条核对。

## 特性

- **免 root / 不污染系统**：所有产物都在安装根；不写 `/usr`，不改 `~/.bashrc`
- **一个项目一个入口**：`./myphp` 自包含，版本和 php.ini 固化成明文，可以进 git
- **多版本并存**：一个 PHP 版本一份 `configs/php-<版本>.conf`，互不影响
- **可搬迁**：整个安装根搬走，`./myphp` 照样能跑（`phprt verify` 会真的拷贝一份来测）
- **可离线**：`downloads/` 放好源码包，`--offline` 全程不联网
- **可选沙箱**：bubblewrap，空根 + 系统库白名单 + 项目只读，只放开指定目录
- **构建可增量**：改了开关只重编影响的模块；`--only php` 单步重编
- **给 agent 的说明书**：加扩展的完整步骤在 [`docs/ADD-EXTENSION.md`](docs/ADD-EXTENSION.md)

## 快速开始

### 0. 前置

需要 C/C++ 工具链（编 PHP 用），沙箱还需要 bubblewrap：

```bash
# Fedora
sudo dnf install gcc gcc-c++ make cmake pkgconf-pkg-config bison perl tar xz unzip curl bubblewrap

# Debian / Ubuntu
sudo apt install build-essential cmake pkg-config bison perl tar xz-utils unzip curl bubblewrap
```

macOS / Windows 没测过（脚本依赖 Linux 的 `flock`、`/proc`、bwrap）。

### 1. 装

```bash
git clone https://github.com/newcloud/phprt.git ~/src/phprt
cd ~/myproject && ~/src/phprt/install.sh
```

交互会问三件事：

1. **装到哪儿** —— `~/.phprt`（全局共用，推荐）或 `./.phprt`（只本项目，删目录即卸载）
2. **PHP 版本** —— 清单在 `lib/versions/`
3. **php.ini** —— `default` / `dev` / `prod` / 已有文件 / 在项目里新建一份

非交互：

```bash
./install.sh --yes                                  # 全默认
./install.sh --global --php 8.5.12 --ini dev
./install.sh --local --ini-local --no-entry         # 装进 ./.phprt，ini 放项目里
./install.sh --offline                              # 只用 downloads/ 里的包
./install.sh --sandbox --rw var                     # 生成的 ./myphp 默认进沙箱
./install.sh --profile minimal                      # 只编零依赖部分（验证/最小环境用）
```

### 2. 用

```bash
./myphp -v
./myphp -m | tr '\n' ' '
./myphp worker.php
./myphp composer install
```

装完后 `build/` 会自动清掉。完整档安装根约 330MB（PHP 204MB + deps 126MB），
另外 `downloads/` 缓存源码包约 196MB（可删，下次要重下）。

## 项目里的 `./myphp`

`./myphp` 是生成的 bash 脚本，开头是固化的信息（`cat` 就能看）：

```bash
PHPRT_ROOT="./.phprt"                        # 安装根（在项目里就是相对路径）
PHPRT_PHP="8.6.0-dev"                        # 用哪个版本
PHPRT_INI="./.phprt/php8.6.0-dev_dev.ini"    # 用哪个 php.ini
PHPRT_ARGS=()

PHPRT_SANDBOX="${PHPRT_SANDBOX:-on}"         # off | on
PHPRT_RW_DIRS="${PHPRT_RW_DIRS:-var var/log}"  # 沙箱里可写的目录
PHPRT_SANDBOX_OPTS="${PHPRT_SANDBOX_OPTS:-}"   # 追加给 bwrap 的参数
PHPRT_SANDBOX_LIBS="${PHPRT_SANDBOX_LIBS:-}"   # 额外放行的系统库
```

依赖库（`deps/*/lib`）和扩展目录是**运行时用通配符找的**，所以重编 PHP、加减依赖
都不需要重新生成这个文件。三个 `PHPRT_*` 变量都能被同名环境变量临时覆盖。

```bash
./myphp --env                 # 看它设了哪些环境变量
./myphp --sandbox-check       # 沙箱自检
./myphp --no-sandbox -v       # 这次不进沙箱
```

## 沙箱（bwrap）

`./myphp` 可以在 [bubblewrap](https://github.com/containers/bubblewrap) 里跑。
目标很朴素：**这个 PHP 只依赖 glibc，那就让它在沙箱里也只看得见 glibc。**

```bash
./myphp --sandbox worker.php                  # 这一次
phprt init --sandbox --rw var                 # 固化进 ./myphp（服务的配置文件不用改）
phprt init --sandbox --rw var --rw var/log    # 可写目录可以列多个
./myphp --sandbox-check                       # 自检
```

沙箱里的世界：

| 项 | 说明 |
| --- | --- |
| 根目录 | **空的**（`--tmpfs /`）：`/home`、`/root`、`/etc` 其余部分、别的项目都不存在 |
| `/usr` | 只读；`/bin /sbin /lib /lib64` 按宿主是软链还是真目录原样处理 |
| 系统库 | `/usr/lib64` **清空后按白名单回填**，系统 `libcurl/libssl/libicu` 直接消失 |
| 项目 | 整体只读（`myphp`、`vendor/`、`.git/` 都改不了） |
| 可写 | 只有 `PHPRT_RW_DIRS` 里的目录（默认 `var`），安装根永远只读 |
| `/tmp`、`/home` | 独立 tmpfs，进程结束即消失 |
| 进程 | 独立 PID 命名空间（PHP 看到自己是 pid 2），看不到宿主进程 |
| 网络 | 默认共享（服务要 bind 端口）；`--no-net` 用 `--unshare-net` |

白名单由 `phprt build` 自动生成到 `<安装根>/php-<版本>/sandbox.libs`
（`ldd` 扫 php + 全部扩展，取安装根之外的路径，再补 NSS / gconv / locale 这些
`ldopen` 才用到的文件），加减依赖会自动跟着变：

```
/lib64/ld-linux-x86-64.so.2     /lib64/libc.so.6
/lib64/libm.so.6                /lib64/libgcc_s.so.1
/usr/lib64/gconv                /usr/lib64/libnss_{files,dns,compat}.so.2
/usr/lib/locale/locale-archive
```

效果是**强制**的：某个扩展如果偷偷链了系统库，沙箱里会当场
`error while loading shared libraries: libxxx`，而不是悄悄跑起来。

```
$ ./myphp --sandbox -r 'echo file_exists("/usr/lib64/libcurl.so.4") ? "可见" : "已挡住";'
已挡住
```

完整参数、逃生口、排错见 **[docs/SANDBOX.md](docs/SANDBOX.md)**。

### 完整例子：RoadRunner 博客

一个真实在跑的项目：RoadRunner 3（Go 写的应用服务器）+ Slim 4 + SQLite，
4 个常驻 PHP worker。**RoadRunner 在沙箱外，worker 在沙箱里**，两者用 pipe 通信。

```
浏览器 ──HTTP──▶ RoadRunner :8080 ──pipes──▶ PHP worker（bwrap 内，Slim 4）──▶ var/blog.sqlite
```

`.rr.yaml`（`server.env` 里的就是沙箱参数）：

```yaml
version: '3'

rpc:
  listen: tcp://127.0.0.1:6001

server:
  command: "./myphp worker.php"     # 项目入口，沙箱由它自己套
  relay: pipes
  env:
    - APP_ENV: dev
    # ---- 沙箱参数 ----
    - PHPRT_SANDBOX: on                    # on | off
    - PHPRT_RW_DIRS: "var var/log"         # 只有这两个目录可写
    - PHPRT_SANDBOX_OPTS: ""               # 追加 bwrap 参数
    - PHPRT_SANDBOX_LIBS: ""               # 额外放行的系统库

http:
  address: 127.0.0.1:8080
  access_logs: true                  # 每个请求一行访问日志
  pool:
    num_workers: 4

logs:
  level: info
  output: var/log/rr.log             # 相对 .rr.yaml 所在目录
```

启动 / 停止（交给用户 systemd，关终端也不会掉）：

```bash
systemd-run --user --unit=blog-rr --collect \
  --working-directory=/home/z/testphp/blog \
  /home/z/testphp/blog/bin/rr serve -c /home/z/testphp/blog/.rr.yaml

systemctl --user restart blog-rr     # 改完代码重启 worker
systemctl --user stop blog-rr
pgrep -x rr                          # 或者按进程名手动 kill（别用 pkill -f）
```

验证它真的在沙箱里：

```bash
$ curl -s localhost:8080/health
{"status":"ok","php":"8.6.0-dev","pid":2,"worker":"c012d2","sandbox":true,"posts":2}
#                          ^ 命名空间里的 pid                        ^ pid 1 是 bwrap

$ pstree -p $(systemctl --user show -p MainPID --value blog-rr)
rr(2881291)─┬─bwrap(2881308)───bwrap(2881348)───php(2881352)
            ├─bwrap(2881309)───bwrap(2881351)───php(2881355)
            ├─bwrap(2881310)───bwrap(2881352)───php(2881356)
            └─bwrap(2881311)───bwrap(2881353)───php(2881357)
```

`.rr.yaml` 里的环境变量优先于 `./myphp` 里的固化值，所以调沙箱只改配置 + 重启：

| 改动 | 结果 |
| --- | --- |
| `PHPRT_RW_DIRS: "var/log"` | `GET /` 200，`POST /post` 500（SQLite 只读，写不进去） |
| `PHPRT_SANDBOX: off` | `/health` → `"pid":2878290,"sandbox":false`（回到宿主 pid） |
| 改回来 + 重启 | `"pid":2,"sandbox":true` |

## 多版本

**一个 PHP 版本一份配置**，都在安装根的 `configs/` 下：

```
<安装根>/configs/
├── php-8.5.12.conf          # 版本 + 开关 + 依赖版本，完整一份
├── php-8.6.0-dev.conf
└── default -> php-8.5.12.conf
```

每个 `php-<版本>/` 带着自己的 stamp、`etc/php.ini`、`extensions/*.so`；
依赖库 `deps/` 跨版本共享（目录名带版本）；ini 是 `php<版本>_{default,dev,prod}.ini`。
**装新版本不影响旧版本。**

```bash
./install.sh --php 8.6.0 --profile minimal   # 只写 configs/php-8.6.0.conf
./install.sh --php 8.6.0 --set-default       # 顺便把 default 指过去
phprt build  --php 8.6.0
phprt verify --php 8.6.0
phprt list                                   # 每版一行：占用、开关数、默认标记

cd myproject && phprt init --php 8.6.0       # 项目切版本（重写 ./myphp 的 PHPRT_PHP）
rm -rf <安装根>/php-8.5.12 <安装根>/php8.5.12_*.ini <安装根>/configs/php-8.5.12.conf
phprt build --prune --yes                    # 清没人引用的依赖
```

配置解析顺序（`lib/common.sh: resolve_config`）：

```
PHPRT_CONFIG=<文件> > --php <版本>（configs/php-<版本>.conf） > configs/default > config.conf（旧布局）
```

老安装第一次跑 `install.sh` 会自动迁移：`config.conf` → `configs/php-<旧版本>.conf`，
`config.conf` 留成指向 `configs/default` 的软链，原有命令不受影响。

| 版本文件 | 版本 | 说明 |
| --- | --- | --- |
| `lib/versions/php-8.5.12.conf` | 8.5.12 | php.net 正式包，完整档（59 模块，204MB） |
| `lib/versions/php-8.6.0-dev.conf` | 8.6.0-dev | GitHub `PHP-8.6` 开发分支，最小档（33 模块，129MB） |

8.6 正式包发布后，加一个 `lib/versions/php-8.6.0.conf`（4 行：版本 / URL / SHA256 / 标签）
就能装，脚本不用改。从 git 分支/归档构建时源码里没有 `configure`，`php.sh` 会自动补
autoconf + re2c 并跑 `buildconf`。

## 扩展

**编译进 php 二进制（静态）**

`openssl` `zlib` `libxml2` `dom` `xml` `xmlreader` `xmlwriter` `SimpleXML` `soap`
`sqlite3` `pdo_sqlite` `curl` `mbstring` `zip` `intl` `gd` `ffi` `mysqli` `pdo_mysql`
`bcmath` `calendar` `exif` `filter` `hash` `iconv` `json` `ctype` `session`
`tokenizer` `Phar` `pcntl` `posix` `sockets` `shmop` `sysvmsg` `sysvsem` `sysvshm` `opcache`

`gd` 支持 **PNG / JPEG / WebP / AVIF** + FreeType 字体。

**PECL 动态扩展（`.so`，在 `php-<版本>/lib/php/extensions/…`）**

| 扩展 | 说明 | ini |
| --- | --- | --- |
| `redis` | Redis 客户端 | `conf.d/20-redis.ini` |
| `apcu` | 进程内共享内存缓存（常驻 worker 友好） | `conf.d/25-apcu.ini` |
| `yaml` | YAML 解析（依赖 libyaml） | `conf.d/26-yaml.ini` |
| `swoole` | 协程/网络框架（C++，需要 g++） | `conf.d/27-swoole.ini` |
| `xdebug` | 调试/覆盖率，默认 `xdebug.mode=off` | `conf.d/30-xdebug.ini` |

调试：`XDEBUG_MODE=debug ./myphp script.php`。

**没编的**：`pgsql`/`pdo_pgsql`（要 libpq）、`imagick`（要 ImageMagick）、
`memcached`/`mongodb`（要外部库）、HEIC（libgd 没这个后端）。

**加新扩展**：完整步骤（模板、必须同步改的 11 处、踩过的坑）见
[`docs/ADD-EXTENSION.md`](docs/ADD-EXTENSION.md)。两条路：

- **编译期扩展**（gd/intl/pgsql 这类）：`config.conf` 加开关 + `lib/modules/<名字>.sh`
  依赖脚本 + `lib/modules/php.sh` 加 configure 参数，然后 `install.sh`（只有 PHP 重编）。
- **PECL 扩展**：照 `lib/modules/redis.sh` 抄 —— 下载 tgz → `phpize` → `configure
  --with-php-config` → `make install` → 往 `php-<版本>/etc/conf.d/` 写一行。

沙箱白名单会自动跟着重编更新（`phprt build` 重新扫 `ldd`），不用手工加。

## 命令参考

```bash
phprt list                           # 已装版本：占用、开关数、默认标记、当前项目用的入口
phprt init [--php 版本] [--ini dev] [--sandbox] [--rw DIR] [--force]
                                     # 在当前目录生成 ./myphp
phprt exec [--php 版本] -- -v        # 不建项目，直接跑一下 PHP
phprt build [--php 版本] [--only php] [--force] [--offline] [--prune] [--jobs N]
phprt verify [--php 版本]            # 自检：结构、依赖记录、ldd、扩展、ini、搬迁、沙箱
phprt clean [--all]                  # 清 build/（--all 连 downloads/）
phprt home                           # 打印当前安装根
```

`install.sh` 直接带 build 参数也行：`./install.sh --yes --offline --jobs 8`。

## 安装根布局

```
~/.phprt/
├── composer.phar              Composer（./myphp composer ... 用它）
├── config.conf -> configs/default
├── configs/
│   ├── php-8.5.12.conf        一个版本一份：版本 + 开关 + 依赖版本
│   ├── php-8.6.0-dev.conf
│   └── default
├── php-8.5.12/                PHP 本体（bin/ lib/ etc/ sandbox.libs .phprt-deps）
├── php-8.6.0-dev/
├── php8.5.12_{default,dev,prod}.ini
├── deps/                      依赖库，跨版本共享（openssl-3.5.9/ icu-79.1/ …）
├── tools/                     构建工具（autoconf、perl 核心模块、re2c）
├── downloads/                 源码包 + SHA256SUMS（可整目录拷走，离线安装）
├── build/                     解压出的源码（可删）
└── logs/                      每次构建一个目录 + history.log
```

仓库里只有脚本：

```
install.sh              用户入口
phprt                   管理命令
build.sh                构建调度
config.conf             开关与依赖版本
ini/                    php.ini 预设
lib/versions/           PHP 版本清单
lib/modules/*.sh        每个依赖一个脚本
lib/myphp-project.tmpl  项目入口 ./myphp 的模板
lib/common.sh           路径、下载、stamp、沙箱白名单生成
lib/verify.sh           自检
docs/                   ADD-EXTENSION.md / SANDBOX.md
downloads/              预下载包（不进 git）
```

## 常见问题

**编译要多久？** 24 核机器上完整档 15–30 分钟（ICU 最久）；最小档 1–2 分钟。
`--only php` 只重编 PHP，约 1–3 分钟。

**能装到别的地方吗？** `./install.sh --prefix /opt/php`，
或 `PHPRT_HOME=/opt/php phprt list`。

**离线怎么装？** 把源码包放进安装根的 `downloads/`（或仓库的 `downloads/` 再 `install.sh`），
然后 `--offline`。包名必须和 URL 最后一段一致。

**下载很慢？** 直连失败会自动走 `http://127.0.0.1:10808`；
换代理：`PHPRT_PROXY=http://127.0.0.1:8080 ./install.sh`；有系统代理时会优先用你的。

**能同时跑两个版本吗？** 能。版本之间只共享 `deps/`，每个 `php-<版本>` 独立；
不同项目各自的 `./myphp` 指向不同版本即可。

**沙箱里缺库怎么办？** 先看 `./myphp --sandbox-check` 和 `phprt verify` 的 ldd 段。
正确做法是把库编进 `deps/`；临时可以用 `PHPRT_SANDBOX_LIBS=/usr/lib64/libxxx.so.N` 放行。

**为什么沙箱里 `getmypid()` 总是 2？** 每个 worker 有独立 PID 命名空间，
`pid 1` 是 bwrap，PHP 就是 2。要区分 worker 用业务层的 id（示例 `/health` 里是 `worker` 字段）。

**会碰系统吗？** 不会。没有 root，不写 `/usr/local`，不改 shell 配置；
`install.sh` 只在安装根和当前目录写文件。

## 文档

- [`docs/SANDBOX.md`](docs/SANDBOX.md) —— 沙箱：挂载表、参数、逃生口、排错
- [`docs/ADD-EXTENSION.md`](docs/ADD-EXTENSION.md) —— 给 agent 的加扩展手册

## License

仓库暂未附带 LICENSE 文件（默认保留所有权利）。要开源给别人用，
建议加一个 MIT 或 Apache-2.0 —— 需要的话提个 issue。
