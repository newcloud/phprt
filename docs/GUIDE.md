# phprt 使用说明

本文是 phprt 的完整用法说明，面向需要选扩展、下载源码、升级或隔离构建的用户。
只想先跑起来，看 [README](../README.md)。

目录：

1. [安装与构建](#1-安装与构建)
2. [只下载源码与离线构建](#2-只下载源码与离线构建)
3. [扩展选择菜单](#3-扩展选择菜单)
4. [扩展清单与编译方式](#4-扩展清单与编译方式)
5. [目录结构与配置](#5-目录结构与配置)
6. [升级 PHP 与依赖更新](#6-升级-php-与依赖更新)
7. [buildroot 隔离构建](#7-buildroot-隔离构建)
8. [单文件 PHP 与全静态](#8-单文件-php-与全静态)
9. [命令速查](#9-命令速查)

## 1. 安装与构建

### 1.1 基本用法

在项目目录里运行：

```bash
cd ~/blog
/path/to/phprt/phprt install 8.5
```

系列写法（`8.5`）用 `bundle.conf` 里的 pin；精确写法（`8.5.12`）装指定版本。
不写 `--ext` 时进入 ANSI 菜单，按分类选择扩展和子选项（见第 3 章）。
脚本或 CI 里加 `--yes`（或不是终端）跳过菜单，用默认集。

也可以直接在命令行指定扩展：

```bash
phprt install 8.5 --ext "openssl curl zip mbstring intl redis"
```

装好后项目里会多一个 `./myphp`，后面都用它跑 PHP：

```bash
./myphp -v
./myphp -m
./myphp worker.php
eval "$(./myphp --shell)"   # 想直接敲 php / composer 时
```

构建成功时 phprt 会打印版本树位置和使用示例：

```text
  ok PHP 8.5.12 就绪 -> /home/you/blog/.phprt/php-8.5.12
    使用示例: ./myphp -r 'var_dump(phpinfo());'
```

如果运行 `install` 时不在项目根目录，成功提示还会多一行 `项目目录: ...`，
到那里执行示例即可。

`./myphp --env` 打印它算出的运行环境变量（PHP、PHPRC、EXT_DIR、CA 路径），
排查问题时用。

### 1.2 扩展相关参数

| 参数 | 作用 |
| --- | --- |
| `--ext "a b"` | 选择扩展；不写时进入交互菜单 |
| `--opt "gd=jpeg,webp"` | 设置扩展的子选项，需要和 `--ext` 一起用 |
| `--build "tidy=static,curl=shared"` | 逐项指定编译方式，优先级高于 `--static` / `--shared` |
| `--static` | 所有支持 static 的扩展编入 PHP；`type=shared` 的保持 shared 并提示 |
| `--shared` | 所有支持 shared 的扩展编成 `.so`；`type=static` 的保持 static 并提示 |

`--opt` 的语义是「列出的非 fixed 子项开启，未列出的关闭」，fixed 子项始终开启。
不写 `--opt` 时用 recipe 里的默认状态。具体子项见 3.4 节。

### 1.3 其他构建选项

| 参数 | 作用 |
| --- | --- |
| `--plan` | 只打印构建计划（依赖、PHP、static/shared 分组），不构建 |
| `--recompile` | 强制重编 PHP（依赖和扩展签名保留） |
| `--clean-src` | 清空已解压源码，重新解压后再编 |
| `--offline` | 只用 `.phprt/downloads`，不联网 |
| `--yes` | 对提示一律回答「是」 |
| `--chroot` / `--host` | 在 buildroot chroot 里构建 / 切回宿主构建，并记为默认 |

已经装好的树会直接提示「已安装」，并打印重新编译的命令：

```bash
phprt install 8.5                 # 已装时只提示，不重复编译
# 输出：
#   ok PHP 8.5.12 已安装，无需重复安装
#       重新编译: phprt install 8.5.12 --recompile
phprt install 8.5 --recompile     # 按需强制重编 PHP
```

要换扩展重新选择可以跑 `phprt select 8.5`。

### 1.4 构建环境要求

构建时需要系统里有 C/C++ 工具链：

```text
gcc gcc-c++ make cmake pkgconf-pkg-config patchelf
tar xz unzip binutils m4 perl curl
```

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

推荐安装 `aria2`（提供 `aria2c`）：多个连接下载，大文件通常会快一些；
没装自动用 curl。下载进度和扩展选择菜单都由 phprt 用 ANSI 自绘，不需要额外的
终端 UI 命令。缺命令时 phprt 会在用到的那一步报 `缺少命令: xxx`。

想完全不碰宿主工具链时用 buildroot，见第 7 章。

## 2. 只下载源码与离线构建

网络不好时，可以先只下载源码，不编译：

```bash
phprt download 8.5 --ext "swoole openssl curl"

# install 也能只下载：只写 .phprt/downloads，不创建版本树、不编译
phprt install 8.5 --ext "swoole openssl curl" --downloads
```

`--downloads` 也接受 `--download` / `--predownload` 两个简写。不写 `--ext` 时
会先进入 ANSI 选择界面，选完再下载；加 `--yes`（或不是终端）直接用默认集。
下载目录固定为项目里的 `.phprt/downloads`。`install`、`add ext`、`update`
也会复用这个目录，文件存在就跳过，不会重复下载。

### 2.1 代理

网络不好时可以配置代理，下载和上游查询都会走它（大小写都认，优先
`https_proxy`）：

```bash
export HTTP_PROXY=http://127.0.0.1:7890
export HTTPS_PROXY="$HTTP_PROXY"
phprt download 8.5 --ext "swoole openssl curl"
```

也可以只对一次命令生效：`phprt download 8.5 --proxy http://127.0.0.1:7890`。

### 2.2 只打印下载链接

```bash
phprt download 8.5 --ext "swoole openssl curl" --print-urls
```

输出格式是「实际缓存文件名 + URL」：

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

### 2.3 并发、超时与进度

下载默认全局 10 路并发，但同一站点最多 2 个；可以用 `--download-jobs N` 和
`--download-host-jobs N` 调整。终端里会先列出所有要下载的包和完整地址，然后按
这两个限制启动；进度区宽度固定，不会随进度数字或 URL 长度变化。进度区只显示
包名、进度、速度和状态，其余显示「等待」，完成一个自动补上下一个。单个文件默认
最多下载 900 秒，可以用 `--timeout N` 调整；超时或连续 60 秒低于 1KB/s 就放弃
该文件，继续下载其它文件。管道或 CI 里自动改成逐行日志，也可以用 `--ui plain`
强制关闭进度。代理只使用用户配置的 `http_proxy`、`https_proxy`（大写也认）或
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

### 2.4 GitHub 镜像回退

GitHub 下载会先直连，失败后按顺序尝试镜像：

```text
https://gh-proxy.com/
https://ghfast.top/
https://ghproxy.net/
https://gh.llkk.cc/
```

直连默认最多等 60 秒（`PHPRT_GITHUB_DIRECT_TIMEOUT`），之后换镜像并从断点续传。
镜像列表可以用 `PHPRT_GITHUB_MIRRORS="镜像1 镜像2"` 覆盖；设为空字符串可关闭。

### 2.5 下载参数与环境变量

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
`Content-Length` 判断。中断后重跑会续传，失败的文件最后单独列出,
不会中断其它下载。

## 3. 扩展选择菜单

`phprt install 8.5` 不写 `--ext` 时，需要构建的情况下会打开内置的选择器。
它是纯 ANSI 自绘的；分类、树形子项、三态勾选和固定项都在同一个终端界面里完成：

```bash
phprt install 8.5                  # 选完扩展再装
phprt select 8.5                   # 只选择并打印，不安装
phprt install 8.5 --yes            # 不询问，直接用默认集（脚本/CI）
```

`phprt select --default` 不进入菜单，直接打印默认集。

### 3.1 键位

选择器里 `↑↓` 移动；跨分类时会先在下一组的分类标题上停住，再按一次才进入组内。
`Home`/`End` 跳到首行/末行（首行是第一个分类标题），`Space` 切换选中，
`Enter` 展开或收起有子项的扩展（在最后一行「确认并继续」上回车就是完成），
`Esc` 取消；在子项或已展开的父行上按 `Esc` 是先收起、再按一次才取消。分类标题
只停靠，`Space`/`Enter` 在标题上不动作。不是终端、`--yes`、`--print-urls` 或
`PHPRT_SELECT=0` 时自动退回默认集。

打开时停在最后的确认行，按 `Home` 回到第一个分类标题。菜单画在当前命令行下面，
不清屏：`$ phprt select 8.5` 那行和上面的历史输出一直看得见，退出后只清掉
菜单这一块。想要全屏菜单可以设 `PHPRT_ALT_SCREEN=1`。

### 3.2 布局与分组

有子项的扩展展开后直接排在下面（节选，中间分类省略）：

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
  [ ] apcu
  [ ] memcached
  [ ] redis ▶

Image Processing
> [-] gd ▼
      [ ] jpeg
      [x] png (固定)
      [ ] webp
      [x] avif
      [x] freetype

  ✔ 确认并继续（18 个扩展）

69/69 · ↑↓ 选择 · Esc 取消 · Home/End 跳转
```

分类标题顶格、用亮青色显示；扩展行缩进 2 格，子选项缩进 6 格；分类之间、
确认行上下各留一个空行。

菜单按 recipe 头部的 `category=` 分组，顺序固定为：Database、Cache & Storage、
Messaging、Network、Runtime、Serialization & Data Formats、Compression、
Cryptography、Image Processing、Internationalization、XML & Documents、Other。
分类标题可以停靠：`↑↓` 跨组时先停在标题上，再按一次进入组内；标题行没有勾选
或展开动作，`Space`/`Enter` 在标题上不动作。

### 3.3 三态标记

父行是三态标记：

- `[x]`：选中；有子项时要求所有非固定子项都开
- `[-]`：选中，但至少有一个非固定子项关着（gd 默认 avif 关，所以默认显示 `[-]`）
- `[ ]`：未选中

`▼` 表示已展开，`▶` 表示已收起；没有子项的扩展没有箭头。固定子项（`png`）
始终是 `[x]`，显示成灰色、Space 不会改动它，也不参与「全开」判断。

当前光标行加粗、蓝底白字（`NO_COLOR` 或 `TERM=dumb` 时改反显）。在父行按
`Space` 切换这个扩展要不要编，重新选中会恢复上次的子项配置；父扩展没选中时
也能展开，子项显示 `[ ]`，在子项上按 `Space` 会自动选中父扩展。

### 3.4 子选项

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

## 4. 扩展清单与编译方式

### 4.1 默认集

`openssl zlib libxml2 curl sqlite3 mysqli pdo_mysql mbstring zip sodium gettext
pcntl posix sockets shmop sysvmsg sysvsem sysvshm`（下表中带 ★）。

### 4.2 编译方式

- **静态**：编进 `bin/php`，启动即用。
- **shared**：编成独立的 `.so`，可以随时停用或换方式。

### 4.3 内置扩展（30 个）

「分类」就是第 3 章菜单里的分组，来源是 recipe 头部的 `category=`。

| 扩展 | 分类 | 用途 | 默认 | 可选 |
| --- | --- | --- | --- | --- |
| bz2 | Compression | bzip2 压缩 | 静态 | 仅静态 |
| curl | Network | HTTP 客户端 | 静态 ★ | 静态 / shared |
| ffi | Other | 调用 C 库 | 静态 | 静态 / shared |
| ftp | Network | FTP 客户端 | 静态 | 静态 / shared |
| gd | Image Processing | 图像处理 | 静态 | 静态 / shared |
| gettext | Internationalization | 国际化翻译 | 静态 ★ | 静态 / shared |
| gmp | Other | 大整数运算 | 静态 | 静态 / shared |
| intl | Internationalization | ICU 国际化 | 静态 | 静态 / shared |
| ldap | Network | LDAP 目录 | 静态 | 静态 / shared |
| libxml2 | XML & Documents | XML / DOM / SimpleXML / SOAP | 静态 ★ | 仅静态 |
| mbstring | Internationalization | 多字节字符串 | 静态 ★ | 静态 / shared |
| mysqli | Database | MySQL 专用扩展（mysqlnd） | 静态 ★ | 仅静态 |
| openssl | Cryptography | 加密 / TLS | 静态 ★ | 仅静态 |
| pcntl | Runtime | 进程控制（fork / 信号） | 静态 ★ | 静态 / shared |
| pdo_mysql | Database | PDO 的 MySQL 驱动（mysqlnd） | 静态 ★ | 仅静态 |
| pdo_pgsql | Database | PDO 的 PostgreSQL 驱动（libpq） | 静态 | 仅静态 |
| pgsql | Database | PostgreSQL 专用扩展（libpq） | 静态 | 仅静态 |
| posix | Runtime | POSIX 接口（用户 / 进程信息） | 静态 ★ | 静态 / shared |
| readline | Runtime | 交互式 CLI 行编辑（`php -a`） | 关 | 仅静态 |
| shmop | Runtime | 共享内存段 | 静态 ★ | 静态 / shared |
| sockets | Network | Socket 接口 | 静态 ★ | 静态 / shared |
| sodium | Cryptography | 现代加密 | 静态 ★ | 静态 / shared |
| sqlite3 | Database | SQLite（含 pdo_sqlite 模块） | 静态 ★ | 仅静态 |
| sysvmsg | Runtime | System V 消息队列 | 静态 ★ | 静态 / shared |
| sysvsem | Runtime | System V 信号量 | 静态 ★ | 静态 / shared |
| sysvshm | Runtime | System V 共享内存 | 静态 ★ | 静态 / shared |
| tidy | XML & Documents | HTML 清理 | 静态 | 静态 / shared |
| xsl | XML & Documents | XSLT | 静态 | 静态 / shared |
| zip | Compression | ZIP 压缩包 | 静态 ★ | 静态 / shared |
| zlib | Compression | gzip / deflate | 静态 ★ | 仅静态 |

### 4.4 PECL 扩展（14 个）

| 扩展 | 分类 | 用途 | 默认 | 可选 |
| --- | --- | --- | --- | --- |
| amqp | Messaging | RabbitMQ | shared | 静态 / shared |
| apcu | Cache & Storage | 用户态缓存 | shared | 静态 / shared |
| igbinary | Serialization & Data Formats | 序列化 | shared | 静态 / shared |
| imagick | Image Processing | ImageMagick 图像 | shared | 静态 / shared |
| memcached | Cache & Storage | Memcached | shared | 静态 / shared |
| mongodb | Database | MongoDB | shared | 静态 / shared |
| msgpack | Serialization & Data Formats | MessagePack | shared | 静态 / shared |
| protobuf | Serialization & Data Formats | Protocol Buffers | shared | 静态 / shared |
| rdkafka | Messaging | Kafka | shared | 静态 / shared |
| redis | Cache & Storage | Redis | shared | 静态 / shared |
| ssh2 | Network | SSH2 | shared | 静态 / shared |
| swoole | Runtime | 协程 / 异步网络 | shared | 仅 shared |
| xdebug | Other | 调试 / 覆盖率 | shared | 仅 shared |
| yaml | Serialization & Data Formats | YAML | shared | 静态 / shared |

数据库相关的扩展拆成独立条目：`mysqli` / `pdo_mysql`、`pgsql` / `pdo_pgsql`、
`sqlite3`（`sqlite3` 一起产出 `sqlite3` 和 `pdo_sqlite` 两个模块）。
`mysqlnd`、`libpq`、`sqlite` 是底层依赖，由构建系统按选择自动计算、去重和构建，
不出现在扩展清单里。

### 4.5 查询扩展

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

recipe 的契约写在第二行 `# phprt:`：除了版本、依赖、编译方式，还可以写
`category=`（选择器分组）、`menu_order=`（组内排序）和 `note=`（语义坑）。
`notes.conf` 把 `note=` 的 slug 映射成人类可读说明，`ext list --json` 会一并输出；
新增 recipe 不需要改 driver。契约格式见 [RECIPE.md](RECIPE.md)。

### 4.6 增删扩展与查看状态

```bash
phprt add ext swoole --shared          # 加一个 shared 扩展，不重编 PHP
phprt add ext gmp --static             # 加一个 static 扩展，会重编 PHP
phprt add ext redis --shared --opt redis=lzf
phprt remove ext swoole                # 停用 shared 扩展
phprt show                             # 当前版本、路径、已启用扩展
phprt show --tree curl                 # 看某个扩展的运行时库依赖树
```

`add ext` 必须先读契约再显式选 `--shared` 或 `--static`：契约里 `type=shared`
的（如 xdebug）只能 `--shared`，`type=static` 的（如 libxml2）只能 `--static`。
已经是同一种方式、又没带 `--opt` 时直接跳过；带 `--opt` 会按新子选项重编。
切到另一种方式同样会重编 PHP。`--opt` 只改点名的扩展，树里其它扩展的子选项
保持不变。`remove ext` 只停用 shared 扩展（删对应的 `conf.d` ini）；static
扩展需要重编 PHP 才能去掉，命令只提示。

## 5. 目录结构与配置

### 5.1 目录

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
- `lib/phprt/`：PHP 和扩展依赖的动态库。文件名统一改成 `libssl-rt3.so.3`
  这种 `-rt` 形式（wheel 方式），运行时只从这里加载，不会用到系统里的同名库。
- `deps/`：第三方库的安装目录，比如 OpenSSL、curl、libxml2。
- `etc/php.ini`：主配置。
- `etc/conf.d/`：扩展和 CA 的配置。
- `etc/ssl/cert.pem`：curl / openssl 用的根证书。

**`build-php-8.5.12/`**：编译过程目录，放源码、日志和构建工具。
不需要时可以直接删，不影响已经装好的 PHP；下次构建会按需重新生成。
构建过程中不要删。

**`downloads/`**：所有源码包。`install`、`add ext`、`update` 会先补齐这里的
文件，再开始编译；文件已存在就跳过。

**`buildroots/`**：可选的 chroot 构建环境，`phprt buildroot` 创建。
不需要时用 `phprt buildroot remove` 删除，不要直接 `rm -rf`，避免残留挂载。

### 5.2 配置

版本树就是一个标准 PHP 安装：主配置在 `etc/php.ini`，扩展开关和 CA 配置在
`etc/conf.d/`，CA 文件在 `etc/ssl/cert.pem`。项目根和 `.phprt/` 根下不放
`php.ini` 和 `conf.d`。

运行时路径全部由 `./myphp` 启动时按自身位置算出来，再通过环境变量或 `-d`
传给 php，所以整棵版本树改名或搬走都不需要重编、不用改配置：

```bash
mv ~/blog ~/blog2
cd ~/blog2 && ./myphp -v
```

`php.ini` 里不写绝对路径；需要路径时引用 `myphp` 导出的环境变量。

## 6. 升级 PHP 与依赖更新

### 6.1 检查上游版本

```bash
phprt outdated            # 只查当前 php.ini 启用的扩展及其依赖
phprt outdated --all      # 查全部已接入模块
phprt outdated --json     # 机器可读结果
```

退出码：0 查询成功，3 网络或上游不可用。

### 6.2 装新的 PHP 补丁版

当前只支持 **PHP 8.5**（8.5.x）。PHP 出了新的补丁版时，直接装新版本：

```bash
phprt outdated
phprt install 8.5.13 --ext "openssl curl zip mbstring intl redis"
phprt use 8.5              # 切 current 到该系列已装的最高版本
phprt list                 # 列出已安装版本，当前版本前标 *
```

### 6.3 依赖库安全更新

```bash
phprt update openssl          # 自动查同系列最新安全版本
phprt update openssl 3.5.10   # 或指定版本
phprt update openssl --dry-run
phprt update openssl --downloads
```

`phprt update` 只更新一个依赖库；需要时相关的 PHP 和扩展会重编。

## 7. buildroot 隔离构建

宿主工具链太杂，或者想要干净可复现的构建环境时，可以在 chroot 里构建。
rootfs 默认放在项目 `.phprt/buildroots/` 里，装 gcc、tar 等编译工具都在 rootfs
内，不影响原系统；用完删掉 rootfs 目录即可。

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
phprt buildroot list
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
- chroot 构建依赖 dnf 系发行版。实现细节见 [CHROOT-BUILD.md](CHROOT-BUILD.md)。

## 8. 单文件 PHP 与全静态

需要分发单个 `bin/php` 时：

```bash
phprt install 8.5 --static-all --ext "openssl zlib curl sqlite3 mbstring"
phprt install 8.5 --static-all-lto --ext "openssl zlib curl sqlite3 mbstring"
```

- `--static-all`：扩展和第三方依赖都静态链接进 `bin/php`；`type=shared` 的扩展
  （如 xdebug）自动排除。glibc 仍动态，产物可单独复制运行。当前支持 openssl
  zlib curl sqlite3 mbstring。
- `--static-all-lto`：在 `--static-all` 基础上给 PHP 本体和依赖都开 LTO；
  状态记录在 `.phprt-state/lto`。

细节分别见 [STATIC-ALL.md](STATIC-ALL.md)、[STATIC-ALL-LTO.md](STATIC-ALL-LTO.md)、
[STATIC-LIBC.md](STATIC-LIBC.md)。

## 9. 命令速查

### 9.1 命令

| 命令 | 作用 |
| --- | --- |
| `phprt install <系列\|版本>` | 安装 PHP（可带 `--ext` / `--opt` / 构建方式） |
| `phprt download <系列\|版本>` | 只下载源码到 `.phprt/downloads` |
| `phprt add ext <扩展> --shared\|--static` | 当前版本树里加扩展 |
| `phprt remove ext <扩展>` | 停用 shared 扩展 |
| `phprt use <系列\|版本>` | 切换 `current` 软链 |
| `phprt list` / `phprt current` | 列出已装版本 / 只打印当前版本 |
| `phprt select [系列]` | 交互选择扩展，只打印结果 |
| `phprt ext list [系列]` | 列出某个系列的全部可用扩展 |
| `phprt show [--tree [扩展]]` | 当前版本、路径、配置、已启用扩展、库依赖树 |
| `phprt outdated [--all] [--json]` | 检查上游版本 |
| `phprt update <依赖> [版本]` | 依赖库安全更新 |
| `phprt verify` | 自检 PHP、扩展、动态库、RPATH、配置路径、CA |
| `phprt buildroot <子命令>` | chroot 构建环境（`create/enter/exec/unmount/remove/status/list/use/env`） |

完整参数用 `phprt --help` 查看。

### 9.2 环境变量

| 变量 | 作用 |
| --- | --- |
| `PHPRT_REPO` | phprt 仓库位置（默认是脚本所在仓库） |
| `PHPRT_WORK` | 构建区基目录（默认 `<项目>/.phprt`） |
| `OFFLINE=1` | 不联网（`outdated` 会直接报告未查询） |
| `http_proxy` / `https_proxy`（大写也认） | 查询和下载使用的代理；未设置时直连 |
| `PHPRT_ARIA2=0` | 禁用 aria2c，强制用 curl |
| `PHPRT_ARIA2_CONNECTIONS` | aria2c 每个文件的连接数（默认 4） |
| `PHPRT_SELECT=0` | 禁用交互菜单；TTY 下退回简单的文本提示 |
| `PHPRT_CHROOT_PASS` | sudo 密码，供 buildroot 命令使用 |

扩展清单由 `bundles/php<系列>/{builtin,ext}/*.sh` 决定；新增一个 recipe 文件
就会出现在可选清单里，不需要改 driver。开发说明见
[DEVELOPMENT.md](DEVELOPMENT.md)。
