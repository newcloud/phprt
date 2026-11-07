# phprt

## 项目介绍

phprt 类似 Python 的 uv，用一条命令为 PHP 项目配置一套隔离系统环境的 PHP 运行环境。

仓库只做软件源，装好的东西全部放在项目自己的 `.phprt/` 里：PHP、依赖库、扩展、源码缓存
都是项目私有的，不往系统目录写文件，不同项目互不影响。运行时不需要设置任何环境变量，
整个 `.phprt` 可以跟着项目一起搬走。

## 前置依赖

构建是源码编译，需要系统里有这些命令。缺少时 `phprt` 会在用到的那一步报
`缺少命令: xxx`。

| 命令 | Fedora / RHEL 包 | 用途 |
| --- | --- | --- |
| `gcc`、`g++` | `gcc`、`gcc-c++` | 编译 PHP、依赖库和扩展（ICU、swoole、re2c 等是 C++） |
| `make` | `make` | 所有构建阶段 |
| `cmake` | `cmake` | libzip、libjpeg-turbo、libwebp、libaom、libavif |
| `pkg-config` | `pkgconf-pkg-config` | libavif、freetype 探测依赖 |
| `patchelf` | `patchelf` | 收尾时改 SONAME / `DT_NEEDED` / RPATH，缺它无法完成构建 |
| `sha256sum` | `coreutils`（系统自带） | 算内容 hash（库改名）和校验源码包 |
| `tar`、`xz`、`unzip` | `tar`、`xz`、`unzip` | 解压 `.tar.gz` / `.tar.xz` / `.zip` |
| `ar` | `binutils` | SQLite 静态库 |
| `m4`、`perl` | `m4`、`perl` | 构建 OpenSSL、autoconf（`perl-interpreter` 缺 `lib.pm`，OpenSSL Configure 会失败） |
| `curl` | `curl` | 仅在 `downloads/` 和缓存里都找不到源码包时才联网下载 |

一次装齐：

```bash
# Fedora / RHEL
sudo dnf install gcc gcc-c++ make cmake pkgconf-pkg-config patchelf \
    tar xz unzip binutils m4 perl curl

# Debian / Ubuntu
sudo apt install build-essential cmake pkg-config patchelf \
    tar xz-utils unzip binutils m4 perl curl

# Arch
sudo pacman -S base-devel cmake pkgconf patchelf \
    tar xz unzip binutils m4 perl curl
```

`patchelf` 和 `sha256sum` 是每次成功构建都要用的，不是可选项。
`tools/` 里的 autoconf、re2c、perl 核心模块会在需要时自动装到 `.phprt/tools/`，
不用手工准备，但它们的构建本身仍依赖上表的 `gcc` / `make` / `m4` / `perl`。

> **注意**：目前只在 Fedora 43 上测试过，其他发行版和版本没有测试；也没在正式生产环境用过。

## 使用示例

```bash
cd /path/to/project

# 首次：选版本和扩展，构建到项目的 .phprt/
/path/to/phprt/phprt set 8.5.12 --ext "openssl zlib libxml2 curl sqlite mysqli mbstring zip redis" --yes

# 使用
./myphp -v
./myphp -m
./myphp worker.php

# 想直接敲 php / composer，把环境导进当前 shell
eval "$(./myphp --shell)"
```

```bash
# 看计划，不构建
phprt set 8.5.12 --ext "sqlite redis" --plan

# 切版本（同一项目可以有多套）
phprt set 8.6.0-dev --ext "openssl zlib sqlite"
phprt current
phprt list

# 加/去一个 PECL 扩展：只编它，PHP 不动
phprt add ext yaml
phprt remove ext yaml

# 自检
phprt verify

# 检查已安装软件有没有同系列新版本（只读，不下载不编译）
phprt outdated

# 强制重编 PHP / 清源码重来
phprt set 8.5.12 --recompile
phprt set 8.5.12 --clean-src
```

支持 `php-8.5.12`、`php-8.6.0-dev`、`php-7.4.33`（7.4 目前只有最小档）。

## Demo：用 chroot 构建（可选）

宿主工具链太杂、或想要干净可复现的构建环境时，可以在 chroot 里跑同一套 `phprt`：

```bash
cd /path/to/project

# 1. 创建 chroot 环境（路径自己定；相对路径按当前目录解析）
/path/to/phprt/phprt buildroot create ../buildrootabc

# 2. 进入（自动挂载 proc/sys/最小 dev；exit 时自动卸载）
/path/to/phprt/phprt buildroot enter ../buildrootabc

# 3. 在 chroot 里按平时的方式构建（例如安装 PHP 8.5，命令见上方「使用示例」）

# 4. 退出后查看状态 / 删除
/path/to/phprt/phprt buildroot status ../buildrootabc
/path/to/phprt/phprt buildroot remove ../buildrootabc
```

- 环境默认跟随当前系统版本（Fedora 43 实测）；`create ../buildrootabc 43` 可指定 releasever
- dnf 包缓存默认在 `/var/cache/dnf-phprt`：删除、重建 rootfs 不用重新下载
- `remove` 只删 `create` 创建过的目录（带 `.phprt-buildroot` 标记），且先卸载、确认 0 挂载
- chroot 内是 root 构建；退出后按需 `sudo chown -R "$(id -u):$(id -g)" .phprt`
- 实测：PHP 8.5.12 + 16 个指定扩展 → 加载 56 个扩展，`phprt verify` 全绿（gd 的 WebP/AVIF 均为 1）

## 特点

**1. wheel 方式管理动态库。** 依赖库的名字换成稳定的运行时名（rt + SONAME 的
ABI 版本号）、SONAME 同步改写，所有消费方的 `DT_NEEDED` 一起改，RPATH 写成
`$ORIGIN` 相对路径。于是同名库不会和系统里的抢，运行时不用 `LD_LIBRARY_PATH`，
整棵树搬走或改名都能跑；同系列小版本升级（`phprt update`）消费方零改写。

**2. 依赖按 builtin / deps / ext / tools 分类。** 内置扩展、第三方库、PECL 扩展、构建工具
各自独立 recipe、独立签名、按依赖拓扑排序，把系统依赖隔离在项目内，只使用树里的库。

**3. downloads 支持离线。** 可以提前把源码包放进 `downloads/`，构建时直接硬链接到项目，
不联网；缺哪个才按 URL 下载。

**4. gd 的图片格式可配。** `gd` 默认支持 PNG / JPEG / AVIF / WebP（PNG 由 libpng 提供，
无法单独关闭）；想关掉某个格式，改 bundle.conf 里的 `ENABLE_GD_JPEG` / `ENABLE_GD_AVIF` /
`ENABLE_GD_WEBP`，重编后生效。`phprt verify` 会核对实际格式支持与开关一致。

## 文档

- [运行时库的 wheel 化](docs/WHEEL-LIBS.md)
- [给 bundle 加扩展或依赖](docs/ADD-EXTENSION.md)
- [从 v1 重构到 v2](docs/REFACTOR-V1-TO-V2.md)
- [项目本地自包含模型](docs/PROJECT-LOCAL.md)
- [多 PHP 版本的配置组织](docs/PHP-VERSIONS.md)
- [chroot 构建环境（可选）](docs/CHROOT-BUILD.md)

## 开发说明

项目由 newcloud 与 AI 助手 DeepSeek V4.1 Flash 协作完成：需求、设计和验收由 newcloud 主导，
代码、文档与实测由双方交替完成。

## 协议

[Mozilla Public License 2.0](LICENSE) © 2026 newcloud

