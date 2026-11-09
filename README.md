# phprt

phprt 是一个给 PHP 项目准备运行环境的构建脚本，用法类似 Python 的 uv。
一条命令装好 PHP、依赖库和扩展，全部放在项目自己的 `.phprt/` 里，不往系统目录
写文件，也不依赖系统里已装的库版本。不同项目互不影响，运行时不用设置环境变量。

## 快速开始

在 blog 项目里：

```bash
cd ~/blog
/path/to/phprt/phprt install 8.5 --ext "openssl curl zip mbstring intl redis"
./myphp -r "phpinfo();"
```

装好后项目里会多一个 `./myphp`，后面都用它跑 PHP：

```bash
./myphp -v
./myphp -m
./myphp worker.php
eval "$(./myphp --shell)"   # 想直接敲 php / composer 时
```

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

## 需要安装的环境

构建时需要系统里有 C/C++ 工具链：

```text
gcc gcc-c++ make cmake pkgconf-pkg-config patchelf
tar xz unzip binutils m4 perl curl
```

Fedora：

```bash
sudo dnf install gcc gcc-c++ make cmake pkgconf-pkg-config patchelf \
    tar xz unzip binutils m4 perl curl
```

缺命令时 phprt 会在用到的那一步报 `缺少命令: xxx`。

## 特点

1. **动态库依赖隔离**：PHP 和扩展运行时只用 `.phprt` 里的库，系统升级 libssl、libcurl 不影响项目。
2. **足够多的扩展支持**：34 个常用内置和 PECL 扩展，默认集开箱即用。
3. **项目自包含、可搬移**：`./myphp` 和 `.phprt` 一起复制到别的目录或机器照常运行。
4. **可直接升级 PHP**：出 8.5.13 时，`phprt install 8.5.13 --ext "..."` 装新版本。
5. **依赖安全更新**：`phprt update openssl` 自动升到同系列最新安全小版本。
6. **扩展可增删**：shared 扩展随时加、随时停用，不用重装 PHP。
7. **可生成单个 php**：`--static-all` / `--static-all-lto` 生成一个 `bin/php`，方便分发（当前支持 openssl zlib curl sqlite mbstring）。
8. **干净构建**：可选 chroot 构建环境，不混入宿主工具链。
9. **离线构建**：提前把源码包放进 phprt 仓库的 `downloads/`，断网时直接构建。

## 支持的扩展

默认集：`openssl zlib libxml2 curl sqlite mysqli mbstring zip sodium gettext`（表中带 ★）。

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
| mysqli | MySQL（mysqli / pdo_mysql） | 静态 ★ | 仅静态 |
| openssl | 加密 / TLS | 静态 ★ | 仅静态 |
| pgsql | PostgreSQL（pgsql / pdo_pgsql） | 静态 | 仅静态 |
| sodium | 现代加密 | 静态 ★ | 静态 / shared |
| sqlite | SQLite（sqlite3 / pdo_sqlite） | 静态 ★ | 仅静态 |
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

## 用 chroot 构建（可选）

宿主工具链太杂，或者想要干净可复现的构建环境时，可以在 chroot 里构建：

```bash
# 方式一：直接让 phprt 在 chroot 里构建
phprt buildroot create          # 默认放在 .phprt/buildroots/default
phprt install 8.5 --chroot --ext "openssl curl zip mbstring intl redis"

# 方式二：进入 chroot 手动操作
phprt buildroot enter
phprt install 8.5 --ext "openssl curl zip mbstring intl redis"
exit

phprt buildroot status
phprt buildroot remove
```

- 默认绑定当前项目和 phprt 仓库。
- rootfs 在项目 `.phprt/buildroots/` 里，`create` 也可以指定名字或路径。
- `--host` 切回宿主构建。
- `create` 需要 sudo 和 dnf，第一次会下载几百 MB。

## 注意事项

- 目前只在 **Fedora 43** 上测试过，其他发行版未验证。
- 还没有在生产环境使用。
- 当前只支持 PHP 8.5。
- 首次构建要下载和编译，耗时较长，构建区占磁盘。
- 构建区可以随时删，删了只影响该版本的增量重建。

## 相关文档

- [开发者文档](docs/DEVELOPMENT.md)
- [单个 PHP 可执行文件](docs/STATIC-ALL.md)
- [全静态 + LTO](docs/STATIC-ALL-LTO.md)
- [全静态 libc](docs/STATIC-LIBC.md)

## 协议

[Mozilla Public License 2.0](LICENSE) © 2026 newcloud
