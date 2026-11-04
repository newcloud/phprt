# phprt

phprt 用来装一套独立的 PHP 环境：不依赖系统里的 PHP 和库，不会改动系统，扩展自己选。

**示例：**

```console
$ cd blog
$ ~/src/phprt/phprt init --php 8.6.0-dev    # 在项目里生成 ./myphp
$ ./myphp -v
PHP 8.6.0-dev (cli) (built: Nov  4 2026 13:28:58) (NTS)

$ ./myphp worker.php             # RoadRunner 用这套 PHP 起 worker
```

---

## 目录

- [特点](#特点)
- [和系统里的 PHP 有什么区别](#和系统里的-php-有什么区别)
- [快速开始](#快速开始)
- [项目里的 `./myphp`](#项目里的-myphp)
- [在真实项目里用：RoadRunner](#在真实项目里用roadrunner)
- [多版本](#多版本)
- [扩展](#扩展)
- [命令参考](#命令参考)
- [安装根布局](#安装根布局)
- [常见问题](#常见问题)
- [文档](#文档)

## 特点

- 每个项目一个 `./myphp`，版本、php.ini、扩展开关都写在项目里
- openssl、curl、icu、libzip 由 phprt 编译，不跟随系统版本
- 一个项目一套（`<项目>/.phprt`），或几个项目共用（`~/.phprt`）
- 8.5 / 8.6 可以同时装，项目级切换
- 可搬迁：整个安装根搬到别的路径，`./myphp` 照样能用
- 可离线：`downloads/` 放好源码包，`--offline` 全程不联网
- 构建可增量：改了开关只重编影响的模块
- 加扩展的完整步骤在 [`docs/ADD-EXTENSION.md`](docs/ADD-EXTENSION.md)

## 和系统里的 PHP 有什么区别

用系统包，或者 phpbrew、php-build 这类源码工具装 PHP，openssl、libzip、icu 要先装到
系统里，PHP 链接的是系统那一份。系统里是 OpenSSL 3.2、项目要 3.5，就得升级系统包；
升级又可能影响机器上别的程序。两个项目需要不同版本时，基本没法同时满足。

phprt 把这些库编在项目目录里，版本由项目配置决定，不往系统里装，也不受系统升级影响。

| | `dnf/apt install php` | phprt |
| --- | --- | --- |
| 依赖来源 | 发行版仓库 | phprt 自己编，放在 `deps/` |
| 系统升级影响 | 换 libcurl/libssl 就可能变行为 | 项目里的库不变 |
| 多版本 | 一般只能一个默认版本 | 8.5 / 8.6 并存，项目级切换 |
| 回滚 | 动系统仓库 | 改 `./myphp` 一行 + 重启进程 |
| 卸载 | 卸载包 | `rm -rf ~/.phprt` |
| 代价 | 随发行版更新 | 自己编（约 20 分钟）、每版 100–200MB |

`phprt verify` 会核对扩展有没有加载、库有没有从系统借。

## 快速开始

### 0. 前置

需要 C/C++ 工具链（编 PHP 用）：

```bash
# Fedora
sudo dnf install gcc gcc-c++ make cmake pkgconf-pkg-config bison perl tar xz unzip curl

# Debian / Ubuntu
sudo apt install build-essential cmake pkg-config bison perl tar xz-utils unzip curl
```

macOS / Windows 没测过（脚本依赖 Linux 的 `flock`、`/proc`）。

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
```

依赖库（`deps/*/lib`）和扩展目录是**运行时用通配符找的**，所以重编 PHP、加减依赖
都不需要重新生成这个文件。

```bash
./myphp --env                 # 看它设了哪些环境变量
```

## 在真实项目里用：RoadRunner

`phprt` 不限定应用服务器：`myphp` 只是一个 `exec php` 的入口，
RoadRunner / Swoole / `php-fpm` 都能直接调它。

实测项目（不在本仓库）：RoadRunner 3（Go 写的应用服务器）+ Slim 4 + SQLite，
4 个常驻 PHP worker，通过 pipe 通信。

```
浏览器 ──HTTP──▶ RoadRunner :8080 ──pipes──▶ PHP worker（Slim 4）──▶ var/blog.sqlite
```

`.rr.yaml` 里只需要把 `server.command` 指向项目入口：

```yaml
version: '3'

rpc:
  listen: tcp://127.0.0.1:6001

server:
  command: "./myphp worker.php"     # 项目入口：版本 / php.ini 都固化在里面
  relay: pipes
  env:
    - APP_ENV: dev

http:
  address: 127.0.0.1:8080
  access_logs: true                  # 每个请求一行访问日志
  pool:
    num_workers: 4                   # 4 个 worker 共享一个常驻进程池

logs:
  level: info
  output: var/log/rr.log             # 相对 .rr.yaml 所在目录
```

启动 / 停止（交给用户 systemd，关终端也不会掉）：

```bash
systemd-run --user --unit=blog-rr --collect \
  --working-directory=/path/to/blog \
  /path/to/blog/bin/rr serve -c /path/to/blog/.rr.yaml

systemctl --user restart blog-rr     # 改完代码重启 worker（worker 是常驻的）
systemctl --user stop blog-rr
pgrep -x rr                          # 或者按进程名手动 kill（别用 pkill -f）
```

进程树就是 RR 管着 4 个 PHP：

```bash
$ pstree -p $(systemctl --user show -p MainPID --value blog-rr)
rr(208495)─┬─php(208511)
           ├─php(208512)
           ├─php(208513)
           └─php(208514)
```

`./myphp --env` 可以核对它实际用的 PHP 版本、php.ini、扩展目录和库搜索路径。

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
PHPRT_CONFIG=<文件> > --php <版本>（configs/php-<版本>.conf） > configs/default
```

老安装第一次跑 `install.sh` 会自动迁移：旧的 `config.conf` → `configs/php-<旧版本>.conf`，
并建好 `configs/default`，原有命令不受影响。

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
| `protobuf` | Protocol Buffers C 运行时（自带 upb） | `conf.d/23-protobuf.ini` |
| `apcu` | 进程内共享内存缓存（常驻 worker 友好） | `conf.d/25-apcu.ini` |
| `yaml` | YAML 解析（依赖 libyaml） | `conf.d/26-yaml.ini` |
| `swoole` | 协程/网络框架（C++，需要 g++） | `conf.d/27-swoole.ini` |
| `xdebug` | 调试/覆盖率，默认 `xdebug.mode=off` | `conf.d/30-xdebug.ini` |

调试：`XDEBUG_MODE=debug ./myphp script.php`。

**没编的**：`pgsql`/`pdo_pgsql`（要 libpq）、`imagick`（要 ImageMagick）、
`memcached`/`mongodb`（要外部库）、HEIC（libgd 没这个后端）。

**加新扩展**：完整步骤（模板、必须同步改的 11 处、踩过的坑）见
[`docs/ADD-EXTENSION.md`](docs/ADD-EXTENSION.md)。两条路：

- **编译期扩展**（gd/intl/pgsql 这类）：`config.conf` 加开关 + `lib/modules/deps/<名字>.sh`
  依赖脚本 + `lib/modules/php.sh` 加 configure 参数，然后 `install.sh`（只有 PHP 重编）。
- **PECL 扩展**：照 `lib/modules/ext/redis.sh` 抄 —— 下载 tgz → `phpize` → `configure
  --with-php-config` → `make install` → 往 `php-<版本>/etc/conf.d/` 写一行。
- **PHP 内置扩展**（zlib/gd/intl 这类）：在 `lib/modules/builtin/<扩展>.sh` 里写
  开关、正负向 configure 参数和 `--needs`；`php.sh` 只负责收集，不再写 if 判断。

## 命令参考

```bash
phprt list                           # 已装版本：占用、开关数、默认标记、当前项目用的入口
phprt init [--php 版本] [--ini dev] [--force] [--name 入口名]
                                     # 在当前目录生成 ./myphp
phprt exec [--php 版本] -- -v        # 不建项目，直接跑一下 PHP
phprt build [--php 版本] [--only php] [--force] [--offline] [--prune] [--jobs N]
phprt modules                        # 节点清单：类别 / 开关 / 依赖 / 状态
phprt verify [--php 版本]            # 自检：结构、依赖记录、ldd、扩展、ini、搬迁
phprt clean [--all]                  # 清 build/（--all 连 downloads/）
phprt home                           # 打印当前安装根
```

`install.sh` 直接带 build 参数也行：`./install.sh --yes --offline --jobs 8`。

## 安装根布局

```
~/.phprt/
├── composer.phar              Composer（./myphp composer ... 用它）
├── configs/
│   ├── php-8.5.12.conf        一个版本一份：版本 + 开关 + 依赖版本
│   ├── php-8.6.0-dev.conf
│   └── default
├── php-8.5.12/                PHP 本体（bin/ lib/ etc/ .phprt-deps）
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
config.conf             开关与依赖版本的模板（install.sh 合并进安装根）
ini/                    php.ini 预设
lib/versions/           PHP 版本清单
lib/modules/deps/*.sh   独立库（装到 <安装根>/deps/）
lib/modules/ext/*.sh    PECL 扩展（装到 php-<版本>/）
lib/modules/builtin/*.sh PHP 内置扩展：开关 + configure 参数 + 依赖（php.sh 收集）
lib/modules/tools/*.sh  构建工具（autoconf / re2c / perl 核心模块）
lib/modules/php.sh      PHP 本体
lib/myphp-project.tmpl  项目入口 ./myphp 的模板
lib/common.sh           路径、下载、stamp、依赖记录
lib/verify.sh           自检
docs/                   ADD-EXTENSION.md
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

**会碰系统吗？** 不会。安装时只写安装根和当前目录，不写 `/usr`，也不改 shell 配置。
不要了，`rm -rf` 那个目录即可。

## 文档

- [`docs/ADD-EXTENSION.md`](docs/ADD-EXTENSION.md) —— 给 agent 的加扩展手册

## License

[Mozilla Public License 2.0](LICENSE) © 2026 newcloud

MPL-2.0 是**文件级** copyleft：

- 改了 `phprt` 自己的文件（或基于它新建的文件），这些文件要按 MPL-2.0 开源；
- 但把它当工具用、嵌进你自己的项目（Larger Work，比如一个 PHP 应用），
  你的项目**不需要**开源，可以用任何协议；
- 附带明确的专利授权（MIT 没有这一条）。
