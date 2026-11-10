# phprt

phprt 是一个给 PHP 项目准备运行环境的构建脚本，用法类似 Python 的 uv。
一条命令装好 PHP、依赖库和扩展，全部放在项目自己的 `.phprt/` 里，不往系统目录
写文件，也不依赖系统里已装的库版本。不同项目互不影响，运行时不用设置环境变量。

## 快速开始

在 blog 项目里：

```bash
cd ~/blog
# 不写 --ext：进入 ANSI 菜单，按分类选择扩展和子选项
/path/to/phprt/phprt install 8.5
# 也可以直接在命令行指定扩展
/path/to/phprt/phprt install 8.5 --ext "openssl curl zip mbstring intl redis"
./myphp -r "phpinfo();"
```

菜单是纯 ANSI 自绘的：分类标题、树形子项、三态勾选，`Space` 切换、
`Enter` 展开子项或确认。脚本/CI 里加 `--yes`（或不是终端）时跳过菜单，用默认集。

装好后项目里会多一个 `./myphp`，后面都用它跑 PHP：

```bash
./myphp -v
./myphp -m
./myphp worker.php
eval "$(./myphp --shell)"   # 想直接敲 php / composer 时
```

## 构建后的目录

安装完成后，项目里大概是这个结构：

```text
<project>/
├── myphp
└── .phprt/
    ├── current -> php-8.5.12
    ├── php-8.5.12/          # 编译好的 PHP
    │   ├── bin/php
    │   ├── lib/phprt/       # 运行时动态库
    │   ├── deps/            # 第三方库
    │   └── etc/             # php.ini、conf.d、证书
    ├── build-php-8.5.12/    # 编译过程
    │   ├── src/             # 解压的源码
    │   ├── logs/            # configure / make 日志
    │   └── tools/           # 构建工具
    ├── downloads/           # 所有源码包（PHP、依赖、扩展）
    │   └── download-urls-php8.5.12-日期.txt
    └── buildroots/          # 可选，chroot 构建环境（需要 sudo）
```

**`php-8.5.12/`**：编译出的 PHP 版本树，实际运行用的就是它。

- `bin/php`：PHP 本体。
- `lib/phprt/`：PHP 和扩展依赖的动态库。文件名统一改成 `libssl-rt3.so.3` 这种 `-rt` 形式（wheel 方式），运行时只从这里加载，不会用到系统里的同名库。
- `deps/`：第三方库的安装目录，比如 OpenSSL、curl、libxml2。
- `etc/php.ini`：主配置。
- `etc/conf.d/`：扩展和 CA 的配置。
- `etc/ssl/cert.pem`：curl / openssl 用的根证书。

**`build-php-8.5.12/`**：编译过程目录，放源码、日志和构建工具。
不需要时可以直接删，不影响已经装好的 PHP；下次构建会按需重新生成。构建过程中不要删。

**`downloads/`**：所有源码包。`install`、`add ext`、`update` 会先补齐这里的文件，再开始编译；文件已存在就跳过。

**`buildroots/`**：可选的 chroot 构建环境，`phprt buildroot` 创建。
不需要时用 `phprt buildroot remove` 删除，不要直接 `rm -rf`，避免残留挂载。

## 支持的 PHP 版本

当前只支持 **PHP 8.5**（8.5.x）。其他系列（8.6、7.4 等）后续再支持。

PHP 出了新的补丁版时，直接装新版本：

```bash
phprt outdated
phprt install 8.5.13 --ext "openssl curl zip mbstring intl redis"
```

依赖库的安全更新：

```bash
phprt update openssl          # 自动查同系列最新安全版本
phprt update openssl 3.5.10   # 或指定版本
phprt update openssl --dry-run
```

`phprt update` 只更新一个依赖库；需要时相关的 PHP 和扩展会重编。

## 提前下载源码

网络不好时，可以先只下载源码，不编译：

```bash
phprt download 8.5 --ext "swoole openssl curl"
```

下载目录固定为项目里的 `.phprt/downloads`。`install`、`add ext`、`update`
也会复用这个目录，文件存在就跳过，不会重复下载。

网络不好时可以配置代理，下载和上游查询都会走它（大小写都认，优先
`https_proxy`）：

```bash
export HTTP_PROXY=http://127.0.0.1:7890
export HTTPS_PROXY="$HTTP_PROXY"
phprt download 8.5 --ext "swoole openssl curl"
```

也可以只对一次命令生效：`phprt download 8.5 --proxy http://127.0.0.1:7890`。

只查看下载链接，不下载：

```bash
phprt download 8.5 --ext "swoole openssl curl" --print-urls
```

输出格式是“实际缓存文件名 + URL”：

```text
#deps-curl curl-8.23.0.tar.xz
https://curl.se/download/curl-8.23.0.tar.xz
#ext-swoole swoole-6.2.5.tgz
https://pecl.php.net/get/swoole-6.2.5.tgz
```

`#` 开头是注释，可以直接保存后用 `wget -i` 或 `aria2c -i` 下载。下载完把文件放进
`.phprt/downloads`，再执行 `phprt install 8.5 --offline`。

`install`、`add ext`、`update` 也接受同样的参数：

```bash
phprt install 8.5 --ext "swoole openssl curl" --downloads
phprt install 8.5 --ext "swoole openssl curl" --downloads-print-urls
phprt add ext swoole --shared --downloads
phprt update openssl 3.5.10 --downloads
```

下载默认全局 10 路并发，但同一站点最多 2 个；可以用 `--download-jobs N` 和
`--download-host-jobs N` 调整。终端里会先列出所有要下载的包和完整地址，然后按这两个限制
启动；进度区宽度固定，不会随进度数字或 URL 长度变化。进度区只显示包名、进度、
速度和状态，其余显示“等待”，完成一个自动补上下一个。单个文件默认最多下载
900 秒，可以用 `--timeout N` 调整；超时或连续 60 秒低于 1KB/s 就放弃该文件，
继续下载其它文件。管道或 CI 里自动改成逐行日志，也可以用 `--ui plain` 强制
关闭进度。代理只使用用户配置的 `http_proxy`、`https_proxy`（大写也认）或
`--proxy URL`；都没有就直连。中断后再次运行会从断点继续。

如果系统里装了 `aria2c`，phprt 会用它替代 curl 下载，每个文件默认 4 个连接，
大文件通常会快一些。没有 `aria2c` 时自动退回 curl。

```bash
# Fedora
sudo dnf install aria2
# Debian / Ubuntu
sudo apt install aria2
```

`PHPRT_ARIA2=0` 可以强制用 curl；`PHPRT_ARIA2_CONNECTIONS=4` 可以调整每个
文件的连接数。

GitHub 下载会先直连，失败后按顺序尝试镜像：

```text
https://gh-proxy.com/
https://ghfast.top/
https://ghproxy.net/
https://gh.llkk.cc/
```

直连默认最多等 60 秒（`PHPRT_GITHUB_DIRECT_TIMEOUT`），之后换镜像并从断点续传。
镜像列表可以用 `PHPRT_GITHUB_MIRRORS="镜像1 镜像2"` 覆盖；设为空字符串可关闭。

下载相关参数和环境变量：

| 参数 / 变量 | 默认 | 作用 |
| --- | --- | --- |
| `--download-jobs N` / `PHPRT_DOWNLOAD_JOBS` | 10 | 全局同时下载的文件数 |
| `--download-host-jobs N` / `PHPRT_DOWNLOAD_HOST_JOBS` | 2 | 同一站点/host 桶的并发数 |
| `--timeout N` / `PHPRT_DOWNLOAD_TIMEOUT` | 900 | 单个文件最长下载秒数 |
| `--ui auto\|plain\|progress` / `PHPRT_UI` | auto | 进度区或逐行日志；非终端自动用 plain |
| `--proxy URL` / `HTTP_PROXY` / `HTTPS_PROXY` / `http_proxy` / `https_proxy` | 直连 | 下载和上游查询共用 |
| `PHPRT_ARIA2=0` | 自动 | 禁用 aria2c，强制 curl |
| `PHPRT_ARIA2_CONNECTIONS` | 4 | aria2c 每个文件的连接数 |
| `PHPRT_GITHUB_DIRECT_TIMEOUT` | 60 | GitHub 直连等待秒数，超时换镜像 |
| `PHPRT_GITHUB_MIRRORS` | 4 个内置镜像 | 覆盖 GitHub 镜像列表；空字符串关闭 |

已经存在的文件会跳过；有 SHA256 时按 SHA256 判断，没有时按远端
`Content-Length` 判断。中断后重跑会续传，失败的文件最后单独列出，不会中断其它下载。

## 需要安装的环境

构建时需要系统里有 C/C++ 工具链：

```text
gcc gcc-c++ make cmake pkgconf-pkg-config patchelf
tar xz unzip binutils m4 perl curl
```

推荐安装 `aria2`（提供 `aria2c`）：多个连接下载，大文件通常会快一些；没装自动用 curl。
下载进度和扩展选择菜单都由 phprt 用 ANSI 自绘，不需要额外的终端 UI 命令。

Fedora：

```bash
sudo dnf install gcc gcc-c++ make cmake pkgconf-pkg-config patchelf \
    tar xz unzip binutils m4 perl curl \
    aria2   # 可选，推荐：aria2c 下载加速
```

Debian / Ubuntu 可以额外安装：

```bash
sudo apt install aria2
```

缺命令时 phprt 会在用到的那一步报 `缺少命令: xxx`。

## 特点

1. **动态库依赖隔离**：PHP 和扩展运行时只用 `.phprt` 里的库，系统升级 libssl、libcurl 不影响项目。
2. **足够多的扩展支持**：44 个常用内置和 PECL 扩展，默认集开箱即用。
3. **项目自包含、可搬移**：`./myphp` 和 `.phprt` 一起复制到别的目录或机器照常运行。
4. **可直接升级 PHP**：出 8.5.13 时，`phprt install 8.5.13 --ext "..."` 装新版本。
5. **依赖安全更新**：`phprt update openssl` 自动升到同系列最新安全小版本。
6. **扩展可增删**：shared 扩展随时加、随时停用，不用重装 PHP。
7. **可生成单个 php**：`--static-all` / `--static-all-lto` 生成一个 `bin/php`，方便分发（当前支持 openssl zlib curl sqlite3 mbstring）。
8. **干净构建**：可选 chroot 构建环境，不混入宿主工具链。
9. **离线构建**：用 `phprt download` 提前把源码放进 `.phprt/downloads`，断网时直接构建。
10. **纯 ANSI 终端界面**：扩展选择（分类、树形子项、三态勾选、固定项）和下载进度都在终端里自绘，不依赖外部 UI 工具。
11. **机器可读契约**：`phprt ext list --json --name <扩展>` 一次给出依赖、模块、ini、flags、opts、hooks 和语义坑。
12. **下载支持**：`phprt download` 可以先把源码下好；队列有全局/单站点并发限制、断点续传、aria2c 加速、GitHub 镜像回退、超时与低速放弃，失败文件单独汇总，进度用 ANSI 自绘。

## 支持的扩展

查看某个 PHP 系列的全部可用扩展：

```bash
phprt ext list 8.5
phprt ext list 8.5 --default
phprt ext list --php 8.5 --names
phprt ext list --installed
phprt ext list 8.5 --json
phprt ext list 8.5 --json --name readline
```

不写系列时用当前版本；没有 current 且仓库只有一个 bundle 时自动用它。
`--json` 适合脚本和生成文档表格；加 `--name` 只输出一个扩展，并包含
`switch/modules/ini/needs_detail/flags/opts/hooks/notes` 等完整契约字段。

recipe 的契约仍在第二行 `# phprt:`：除了版本、依赖、编译方式，现在还可以写
`category=`（选择器分组）、`menu_order=`（组内排序）和 `note=`（语义坑）。
`notes.conf` 把 `note=` 的 slug 映射成人类可读说明，`ext list --json` 会一并输出；
新增 recipe 不需要改 driver。

### 交互选择扩展

`phprt install 8.5` 不写 `--ext` 时，需要构建的情况下会打开内置的选择器。
它是纯 ANSI 自绘的；分类、树形子项、三态勾选和固定项都在同一个终端界面里完成：

```bash
phprt install 8.5                  # 选完扩展再装
phprt select 8.5                   # 只选择并打印，不安装
phprt install 8.5 --yes            # 不询问，直接用默认集（脚本/CI）
```

已经装好的树会直接提示“已安装”，不会重新询问；要重新选可以跑
`phprt select 8.5`，或加 `--recompile` 再安装。

选择器里 `↑↓` 移动，`Home`/`End` 跳到首项/末项，`Space` 切换选中，
`Enter` 展开或收起有子项的扩展（在最后一行「确认并继续」上回车就是完成），
`Esc` 取消；在子项或已展开的父行上按 `Esc` 是先收起、再按一次才取消。不是终端、`--yes`、
`--print-urls` 或 `PHPRT_SELECT=0` 时自动退回默认集。

打开时停在最后的确认行，按 `Home` 回到第一个扩展。菜单画在当前命令行下面，
不清屏：`$ phprt select 8.5` 那行和上面的历史输出一直看得见，退出后只清掉
菜单这一块。想要全屏菜单可以设 `PHPRT_ALT_SCREEN=1`。

有子项的扩展展开后直接排在下面：

```text
选择要编译的扩展 · Space 切换 · Enter 展开/收起
Database
  [x] mysqli
  [x] pdo_mysql
  [ ] pdo_pgsql
  [ ] pgsql
  [x] sqlite3
  [ ] mongodb

Cache & Storage
  [ ] memcached
  [ ] redis ▶

Compression
  [x] zip
  [x] zlib

Image Processing
> [-] gd ▼
      [ ] jpeg
      [x] png (固定)
      [ ] webp
      [x] avif
      [x] freetype

  ✔ 确认并继续（18 个扩展）

63/63 · Esc 取消 · Home/End 跳转
```

分类标题顶格、用亮青色显示；扩展行缩进 2 格，子选项缩进 6 格；分类之间、确认行上下各留一个空行。

菜单按 recipe 头部的 `category=` 分组，顺序固定为：Database、Cache & Storage、
Compression、Cryptography、Image Processing、Internationalization、Network、
Runtime、Other。分类标题不是可选项，`↑↓`、`Home/End`、`PgUp/PgDn` 会跳过它们。

父行是三态标记：

- `[x]`：选中；有子项时要求所有非固定子项都开
- `[-]`：选中，但至少有一个非固定子项关着（gd 默认 avif 关，所以默认显示 `[-]`）
- `[ ]`：未选中

`▼` 表示已展开，`▶` 表示已收起；没有子项的扩展没有箭头。固定子项（`png`）
始终是 `[x]`，显示成灰色、Space 不会改动它，也不参与“全开”判断。

当前光标行加粗、蓝底白字（`NO_COLOR` 或 `TERM=dumb` 时改反显）。在父行按
`Space` 切换这个扩展要不要编，重新选中会恢复上次的子项配置；父扩展没选中时
也能展开，子项显示 `[ ]`，在子项上按 `Space` 会自动选中父扩展。

gd 默认启用 `jpeg png webp freetype`；AVIF 默认关闭。PNG 是 PHP 8.5 gd 的
硬性依赖，不能关闭。脚本里可以用 `--opt` 直接指定：

```bash
phprt install 8.5 --ext "gd" --opt gd=jpeg,png,webp,freetype
phprt install 8.5 --ext "gd" --opt gd=jpeg,png
phprt install 8.5 --ext "gd" --opt gd=jpeg,png,webp,avif,freetype
```

`--opt` 的语义是「列出的非 fixed 子项开启，未列出的关闭」；fixed 子项始终开启。
不写 `--opt` 时用 recipe 里的默认状态。

其它扩展也有子项（括号里是默认值）：

| 扩展 | 子项 | 说明 |
| --- | --- | --- |
| curl | `sftp`（关） | SFTP/SCP，会多编一个 libssh2 |
| zip | `bzip2`（关） | libzip 的 bzip2 后端 |
| redis | `lzf`（关） | LZF 压缩，用 redis 自带的 FastLZ，不引入外部库 |
| openssl | `legacy_provider`（关） | 默认加载 legacy provider，兼容老算法/老服务端 |

```bash
phprt install 8.5 --ext "curl zip" --opt "curl=sftp zip=bzip2"
phprt add ext redis --shared --opt redis=lzf
```

`add ext --opt` 会按新子项重编这个扩展（static 的需要重编 PHP），
树里其它扩展的子项保持不变。

默认集：`openssl zlib libxml2 curl sqlite3 mysqli pdo_mysql mbstring zip sodium gettext pcntl posix sockets shmop sysvmsg sysvsem sysvshm`（表中带 ★）。

数据库相关的扩展拆成独立条目：`mysqli` / `pdo_mysql`、`pgsql` / `pdo_pgsql`、`sqlite3`
（`sqlite3` 一起产出 `sqlite3` 和 `pdo_sqlite` 两个模块）。`mysqlnd`、`libpq`、`sqlite`
是底层依赖，由构建系统按选择自动计算、去重和构建，不出现在扩展清单里。

编译方式：

- **静态**：编进 `bin/php`，启动即用。
- **shared**：编成独立的 `.so`，可以随时停用或换方式。

### PHP 内置扩展

| 扩展 | 用途 | 默认 | 可选 |
| --- | --- | --- | --- |
| bz2 | bzip2 压缩 | 静态 | 仅静态 |
| curl | HTTP 客户端 | 静态 ★ | 静态 / shared |
| ffi | 调用 C 库 | 静态 | 静态 / shared |
| ftp | FTP 客户端 | 静态 | 静态 / shared |
| gd | 图像处理 | 静态 | 静态 / shared |
| gettext | 国际化翻译 | 静态 ★ | 静态 / shared |
| gmp | 大整数运算 | 静态 | 静态 / shared |
| intl | ICU 国际化 | 静态 | 静态 / shared |
| ldap | LDAP 目录 | 静态 | 静态 / shared |
| libxml2 | XML / DOM / SimpleXML / SOAP | 静态 ★ | 仅静态 |
| mbstring | 多字节字符串 | 静态 ★ | 静态 / shared |
| mysqli | MySQL 专用扩展（mysqlnd） | 静态 ★ | 仅静态 |
| openssl | 加密 / TLS | 静态 ★ | 仅静态 |
| pcntl | 进程控制（fork / 信号） | 静态 ★ | 静态 / shared |
| pdo_mysql | PDO 的 MySQL 驱动（mysqlnd） | 静态 ★ | 仅静态 |
| pdo_pgsql | PDO 的 PostgreSQL 驱动（libpq） | 静态 | 仅静态 |
| pgsql | PostgreSQL 专用扩展（libpq） | 静态 | 仅静态 |
| posix | POSIX 接口（用户 / 进程信息） | 静态 ★ | 静态 / shared |
| readline | 交互式 CLI 行编辑（`php -a`） | 关 | 仅静态 |
| shmop | 共享内存段 | 静态 ★ | 静态 / shared |
| sockets | Socket 接口 | 静态 ★ | 静态 / shared |
| sodium | 现代加密 | 静态 ★ | 静态 / shared |
| sqlite3 | SQLite（含 pdo_sqlite 模块） | 静态 ★ | 仅静态 |
| sysvmsg | System V 消息队列 | 静态 ★ | 静态 / shared |
| sysvsem | System V 信号量 | 静态 ★ | 静态 / shared |
| sysvshm | System V 共享内存 | 静态 ★ | 静态 / shared |
| tidy | HTML 清理 | 静态 | 静态 / shared |
| xsl | XSLT | 静态 | 静态 / shared |
| zip | ZIP 压缩包 | 静态 ★ | 静态 / shared |
| zlib | gzip / deflate | 静态 ★ | 仅静态 |

### PECL 扩展

| 扩展 | 用途 | 默认 | 可选 |
| --- | --- | --- | --- |
| amqp | RabbitMQ | shared | 静态 / shared |
| apcu | 用户态缓存 | shared | 静态 / shared |
| igbinary | 序列化 | shared | 静态 / shared |
| imagick | ImageMagick 图像 | shared | 静态 / shared |
| memcached | Memcached | shared | 静态 / shared |
| mongodb | MongoDB | shared | 静态 / shared |
| msgpack | MessagePack | shared | 静态 / shared |
| protobuf | Protocol Buffers | shared | 静态 / shared |
| rdkafka | Kafka | shared | 静态 / shared |
| redis | Redis | shared | 静态 / shared |
| ssh2 | SSH2 | shared | 静态 / shared |
| swoole | 协程 / 异步网络 | shared | 仅 shared |
| xdebug | 调试 / 覆盖率 | shared | 仅 shared |
| yaml | YAML | shared | 静态 / shared |

### 最近一轮新增

| 类型 | 新增 | 说明 |
| --- | --- | --- |
| builtin | `pcntl` `posix` `readline` `sockets` `shmop` `sysvmsg` `sysvsem` `sysvshm` | 进程/信号、POSIX、行编辑、socket、共享内存和 System V IPC |
| builtin 拆分 | `mysqli` / `pdo_mysql`、`pgsql` / `pdo_pgsql`、`sqlite3` | 每个驱动独立选择；`sqlite3` 同时产出 `pdo_sqlite` |
| dep | `ncurses` `readline` `libpq` `sqlite` | readline 的 termcap、PostgreSQL 客户端库、SQLite 本体 |
| PECL | `amqp` `imagick` `memcached` `mongodb` `rdkafka` `ssh2` `tidy` 等 | 按需选择，默认集不包含 |

数据库扩展拆开后，`mysqlnd`、`libpq`、`sqlite` 由依赖解析自动构建和去重，
不出现在扩展清单里；`readline` 目前只能静态编译。

## 用 chroot 构建（可选）

宿主工具链太杂，或者想要干净可复现的构建环境时，可以在 chroot 里构建。

```bash
# 创建 chroot 环境：需要 sudo；也可以先用 PHPRT_CHROOT_PASS 传密码
# 默认放在 .phprt/buildroots/default
phprt buildroot create

# 方式一：phprt 自动进入 chroot 构建，结束自动退出并卸载
phprt install 8.5 --chroot --ext "openssl curl zip mbstring intl redis"
phprt add ext swoole --shared --chroot

# 方式二：进入 chroot 手动操作，exit 时自动卸载
phprt buildroot enter
phprt install 8.5 --ext "openssl curl zip mbstring intl redis"
exit

# 查看状态；不需要时删除
phprt buildroot status
phprt buildroot remove
```

- `create`、`--chroot`、`enter`、`remove` 都需要 sudo。可以提前传密码：
  `PHPRT_CHROOT_PASS='...' phprt buildroot create`。
- `--chroot` 只包住构建过程：自动挂载、进入 chroot、构建，结束后自动卸载，
  不需要手动 `unmount`。`add ext` 仍要带 `--shared` 或 `--static`。
- `buildroots/` 不需要时用 `phprt buildroot remove` 删除。不要直接 `rm -rf`，
  避免留下挂载。
- rootfs 在项目 `.phprt/buildroots/` 里，`create` 也可以指定名字或路径。
- `--host` 切回宿主构建。
- 第一次 `create` 会下载几百 MB。

## 注意事项

- 目前只在 **Fedora 43** 上测试过，其他发行版未验证。
- 还没有在生产环境使用。
- 当前只支持 PHP 8.5。
- 首次构建要下载和编译，耗时较长，构建区占磁盘。
- 构建区可以随时删，删了只影响该版本的增量重建。

## 开发说明

项目由 newcloud 与 DeepSeek V4.1 Flash 1017 持续开发。欢迎加入。

新增扩展可以用 `templates/` 里的 5 个模板；契约改动先改
`runner/contract.sh` 的字段表，再跑 `tests/contract/run.sh` 和
`scripts/recipe-check.sh`。机器可读契约用
`phprt ext list 8.5 --json --name <扩展>` 查看。

## 相关文档

- [开发者文档](docs/DEVELOPMENT.md)
- [单个 PHP 可执行文件](docs/STATIC-ALL.md)
- [全静态 + LTO](docs/STATIC-ALL-LTO.md)
- [全静态 libc](docs/STATIC-LIBC.md)

## 协议

[Mozilla Public License 2.0](LICENSE) © 2026 newcloud
