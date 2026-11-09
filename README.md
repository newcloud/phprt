# phprt

## 项目介绍

phprt 类似 Python 的 uv，用一条命令为 PHP 项目配置一套隔离系统环境的 PHP 运行环境。

仓库只做软件源。装好的运行环境放在项目自己的 `.phprt/` 里：PHP、依赖库、扩展，
不往系统目录写文件，不同项目互不影响。运行时不需要设置任何环境变量，
`.phprt` 里的版本树可以跟着项目一起搬走。

构建期文件不往项目根里塞，按 PHP 版本收在 `.phprt/build-php-<版本>/`
（随时可删，删了只影响该版本的增量重建）。源码包以 phprt 仓库的 `downloads/`
为种子；仓库没有时联网下载，存到对应版本的 `build-php-<版本>/downloads/`。

## 前置依赖

构建是源码编译，需要系统里有这些命令。缺少时 `phprt` 会在用到的那一步报
`缺少命令: xxx`。

| 命令 | Fedora / RHEL 包 | 用途 |
| --- | --- | --- |
| `gcc`、`g++` | `gcc`、`gcc-c++` | 编译 PHP、依赖库和扩展（swoole 是 C++） |
| `make` | `make` | 所有构建阶段 |
| `cmake` | `cmake` | libzip、libjpeg-turbo、libwebp、libaom、libavif |
| `pkg-config` | `pkgconf-pkg-config` | libavif、freetype 探测依赖 |
| `patchelf` | `patchelf` | 收尾时改 SONAME / `DT_NEEDED` / RPATH，缺它无法完成构建 |
| `sha256sum` | `coreutils`（系统自带） | 算内容 hash（库改名）和校验源码包 |
| `tar`、`xz`、`unzip` | `tar`、`xz`、`unzip` | 解压 `.tar.gz` / `.tar.xz` / `.zip` |
| `ar` | `binutils` | SQLite 静态库 |
| `m4`、`perl` | `m4`、`perl` | 构建 OpenSSL、autoconf（`perl-interpreter` 缺 `lib.pm`，OpenSSL Configure 会失败） |
| `curl` | `curl` | 仅在 phprt 仓库 `downloads/` 和缓存里都找不到源码包时才联网下载 |

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
`tools/` 里的 autoconf、perl 核心模块会在需要时自动装到当前版本的
`.phprt/build-php-<版本>/tools/`，
不用手工准备，但它们的构建本身仍依赖上表的 `gcc` / `make` / `m4` / `perl`。

> **注意**：目前只在 Fedora 43 上测试过，其他发行版和版本没有测试；也没在正式生产环境用过。

## 使用示例

```bash
cd /path/to/project

# 首次：选系列和扩展，构建到项目的 .phprt/
# 系列写法使用 bundle.conf 的 pin（不联网查 php.net）
/path/to/phprt/phprt install 8.5 --ext "openssl curl zip swoole" --yes

# 使用
./myphp -v
./myphp -m
./myphp worker.php

# 想直接敲 php / composer，把环境导进当前 shell
eval "$(./myphp --shell)"
```

```bash
# 装精确版本 / 看计划不构建
phprt install 8.5.12
phprt install 8.5 --ext "openssl curl zip swoole" --plan

# 切换已装版本（同一项目可以有多套）；系列写法取已装最高版
phprt use 8.5
phprt use 8.5.12
phprt show
phprt current
phprt list

# 加/去一个扩展：只编它，PHP 不动
phprt add ext swoole
phprt remove ext swoole

# 检查上游版本：默认只看当前启用的扩展及其依赖；--all 看全部已接入模块
phprt outdated
phprt outdated --all
phprt outdated --json
OFFLINE=1 phprt outdated     # 全局离线模式：不查询

# 自检
phprt verify

# 强制重编 PHP / 清源码重来
phprt install 8.5 --recompile
phprt install 8.5 --clean-src
```

这份 `phprtv2` 只保留 PHP 8.5。默认扩展集是
`openssl zlib libxml2 curl sqlite mysqli mbstring zip sodium gettext`，
可选清单由 `bundles/php8.5/{builtin,ext}/*.sh` 决定（当前 34 个扩展）；
新增扩展只要加一个 recipe 文件。

### 命令一览

| 命令 | 作用 |
| --- | --- |
| `phprt install <系列\|版本>` | 安装 PHP 和扩展。`8.5` 用 pin，`8.5.12` 装指定版本 |
| `phprt install ... --build "a=static,b=shared"` | 逐项指定编译方式 |
| `phprt install ... --static` / `--shared` | 全局指定；不支持的类型自动保持原方式并提示 |
| `phprt install ... --static-all` | 扩展和依赖都静态链接，生成单个 `bin/php`；`type=shared` 自动排除 |
| `phprt install ... --static-all-lto` | 在 `--static-all` 基础上给 PHP 和依赖都开 LTO |
| `phprt add ext <名字> --shared\|--static` | 加一个扩展；必须显式选方式，static 会重编 PHP |
| `phprt remove ext <名字>` | 停用 shared 扩展（删 conf.d ini）；static 需重编 |
| `phprt use <系列\|版本>` | 切换 current；系列取已装最高版 |
| `phprt show` | 当前版本、路径、php.ini、已启用扩展 |
| `phprt current` | 只打印当前版本号 |
| `phprt list` | 列出已安装版本，当前标 `*` |
| `phprt outdated [--all] [--json]` | 检查上游版本；默认只看已启用的扩展及依赖 |
| `phprt verify` | 自检 PHP、扩展、库、RPATH、配置、CA、wheel |

`install` 选项：`--ext "a b"`、`--build "a=static,b=shared"`、`--static`、`--shared`、
`--static-all`、`--static-all-lto`、`--plan`、`--recompile`、`--clean-src`、`--yes`。
`--static-all` 把扩展和第三方依赖都链接进 `bin/php`，glibc 仍动态；当前先支持
`openssl zlib curl sqlite mbstring`，见 [docs/STATIC-ALL.md](docs/STATIC-ALL.md)。
`--static-all-lto` 在此基础上给 PHP 本体和依赖都加 `-flto=auto`，见
[docs/STATIC-ALL-LTO.md](docs/STATIC-ALL-LTO.md)。
`add ext` 必须带 `--shared` 或 `--static`；不带参数会打印契约支持的 `type` 和 `default`。
完整说明用 `phprt --help`。

## 目录结构

```text
<project>/
├── myphp                  # 项目入口；运行时只需要它和 .phprt/current
└── .phprt/
    ├── current -> php-8.5.12
    ├── php-8.5.12/        # 标准 PHP 安装树（bin/etc/lib/deps）
    ├── php-8.5.11/
    ├── build-php-8.5.12/  # 8.5.12 的构建区；删掉只影响它的增量重建
    │   ├── src/           # 解压的源码和编译对象
    │   ├── logs/          # configure / make 日志
    │   ├── tools/         # autoconf、perlshim（构建期用）
    │   ├── downloads/     # 源码包（下载时写这里）
    │   └── .lock          # flock 构建锁
    └── build-php-8.5.11/  # 同上，两个版本的构建互不干扰
```

构建目录按版本分开，名字直接对应版本树：`php-8.5.12` 配 `build-php-8.5.12`。
删掉某一版的构建目录不影响另一版，也不影响运行。一份完整构建区约 1.1G
（依赖源码/对象 420M + PHP、swoole 源码/对象），删掉后下次构建再重新生成。

`.phprt` 里不保存“启用了哪些扩展”的状态文件：需要时直接问目标版本树的
`php -m`（用和 `myphp` 相同的 `PHPRC` / `PHP_INI_SCAN_DIR` / `extension_dir`）。
所以用户手改 `php.ini`、删掉某个 `.so`、注释掉 `extension=`，`phprt show`
和 `phprt verify` 看到的都是真实结果；不同版本各读各的，互不覆盖。

源码包默认从 phprt 仓库的 `downloads/` 取（硬链接，不额外占空间）；
仓库没有时才联网下载，写进对应版本的 `build-php-<版本>/downloads/`。可用环境变量调整：

| 变量 | 作用 |
| --- | --- |
| `PHPRT_REPO` | phprt 仓库位置（不设则按入口脚本位置推导） |
| `PHPRT_DOWNLOADS` | 强制指定源码缓存目录 |
| `PHPRT_SEED` | 额外的离线源码目录（只读） |
| `PHPRT_CACHE` | 用户缓存根（默认 `~/.cache/phprt`） |
| `PHPRT_WORK` | 构建基目录（默认 `.phprt`，实际目录为 `<基目录>/build-php-<版本>`） |
| `PHPRT_PROXY` | 直连失败时用的代理（默认 `http://127.0.0.1:10808`） |

## 特点

**1. wheel 方式管理动态库。** 依赖库的名字换成稳定的运行时名（rt + SONAME 的
ABI 版本号）、SONAME 同步改写，所有消费方的 `DT_NEEDED` 一起改，RPATH 写成
`$ORIGIN` 相对路径。于是同名库不会和系统里的抢，运行时不用 `LD_LIBRARY_PATH`，
整棵树搬走或改名都能跑。

**2. 依赖按 builtin / deps / ext / tools 分类。** 内置扩展、第三方库、PECL 扩展、构建工具
各自独立 recipe、独立签名、按依赖拓扑排序，把系统依赖隔离在项目内，只使用树里的库。

**3. downloads 支持离线。** 可以提前把源码包放进 phprt 仓库的 `downloads/`
（或 `PHPRT_SEED` 指定目录），构建时直接用，不联网；缺哪个才按 URL 下载，
并存到当前版本的 `.phprt/build-php-<版本>/downloads/`。也可以用 `PHPRT_DOWNLOADS`
指定独立源码缓存。

**4. recipe 即数据，driver 即执行器。** 扩展/依赖/工具都是 `bundles/php8.5/` 下的
recipe 文件（头部一行 `# phprt:` 声明字段，`build()`/`flags()`/`needs()`/`enable()`
是回调）；加扩展只加一个 recipe 文件，不改 driver。规范见
[docs/RECIPE.md](docs/RECIPE.md)，实测见 [docs/TEST-V2.md](docs/TEST-V2.md)。

## 文档

- [单个 PHP 可执行文件（--static-all）](docs/STATIC-ALL.md)
- [全静态 + LTO（--static-all-lto）](docs/STATIC-ALL-LTO.md)
- [recipe 规范与审计](docs/RECIPE.md)
- [v2 开发任务](docs/ROADMAP-V2.md)
- [运行时库的 wheel 化](docs/WHEEL-LIBS.md)
- [phprtv2 实测记录](docs/TEST-V2.md)

## 开发说明

项目由 newcloud 与 AI 助手 DeepSeek V4.1 Flash 协作完成：需求、设计和验收由 newcloud 主导，
代码、文档与实测由双方交替完成。

新增或修改扩展/依赖 recipe 后，先跑契约检查，通过才算添加成功：

```bash
scripts/recipe-check.sh           # 静态检查
scripts/recipe-check.sh --deep    # 再联网确认上游版本（环境允许时）
```

这是开发时的检查脚本，不挂在 `phprt` 命令上，普通用户不需要执行。

## 协议

[Mozilla Public License 2.0](LICENSE) © 2026 newcloud

