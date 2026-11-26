# phprt

**Declarative source builder for PHP and nginx.**

phprt 用源码编译 PHP 和 nginx。产品、组件、依赖库和构建工具统一声明，自动求解依赖线、
补丁和构建顺序；默认把结果装到当前项目里，也支持全局安装。

## 功能

- **声明式构建** — 声明产品和组件，自动求解依赖关系、兼容版本和构建顺序。
- **组件管理** — 独立添加、移除 PHP 扩展、PECL 扩展和 nginx 模块，支持 static / shared
  等编译方式，以及组件自己的 feature 开关。
- **运行时库随产物** — 非系统库收进安装树的 `lib/phprt`，用 `$ORIGIN` rpath 定位；
  glibc 等系统库仍由运行环境提供。
- **可复现的版本选择** — `scheme --lock` 导出组件、补丁和依赖版本，在别的项目或机器上
  重建；不锁 gcc 和构建工具。
- **OCI 构建环境** — 把 gcc / make / patchelf 等装进独立实例，用镜像里的工具编译；
  不依赖 podman / docker / root。

## 安装 phprt

```bash
cargo build --release
cp target/release/phprt ~/.local/bin/      # 或者直接用 ./target/release/phprt
```

- 需要 Linux、Rust（只用来编 phprt），以及 gcc / g++ / make / pkg-config / perl /
  m4 / patchelf。缺命令时 phprt 会自己构建 catalog 里有的工具，其余按发行版给出
  安装命令。
- 在别的项目里使用，要把 recipe 目录指过来：在 `phprt.toml` 里配 `recipes`，或显式传
  `--recipes /path/to/phprtv3/recipes`，见下面的配置。

## 用法

### 安装 PHP / nginx

```bash
phprt install php@8.5

./myphp-8.5 -v
./myphp-8.5 -m
```

默认装到 `.phprt/php-8.5.13/`，源码下载到 `.phprt/downloads/`，解压到 `build/`。
首次运行会下载并编译，不是秒装；中途失败或中断，重跑同一条命令会接着装。

需要 FPM 时：

```bash
phprt install php@8.5 --targets="cli,fpm"
./.phprt/php-8.5.13/sbin/php-fpm -v
```

`--targets` 会写进 state，之后不用重复传；改目标或配置后重装需要 `--force`。见
[docs/TARGETS.md](docs/TARGETS.md)。

nginx 同理：

```bash
phprt install nginx@1.29
.phprt/nginx-1.29.8/sbin/nginx -V
```

当前可装 PHP 7.4 / 8.0 ~ 8.6 和 nginx 1.29；PHP 8.6 用 8.6.0RC3 构建。
nginx 不生成入口，直接用安装树里的二进制。

### 管理扩展

```bash
# 添加、移除扩展
phprt add php@8.5 swoole
phprt remove php@8.5 swoole

# 内置扩展必须写编译方式
phprt add php@8.5 'gd=mode:shared'

# 指定组件版本线
phprt add php@8.5 'swoole=version:6.2'
```

`add` 会改方案并立即编译；该系列还没装过时只记方案，提示先 `install`。带依赖 feature
的写法、磁盘一致性检查和 ini 备份规则见 [docs/FLOW.md](docs/FLOW.md)。

### 升级、查看与复现

```bash
phprt update php@8.5            # 跟随 catalog 升到 8.5 线最新补丁
phprt list php@8.5              # 组件、编译方式、版本、状态
phprt plan php@8.5              # 只看构建计划和依赖顺序

phprt scheme php@8.5            # 导出可复制的重建命令
phprt scheme php@8.5 --lock     # 连补丁和依赖版本一起导出
```

把 `scheme` 的输出拿到另一台机器或另一个项目执行，就得到同一套组件、编译方式和构建
目标。`--lock` 需要 catalog 里仍有对应旧补丁；它不锁 gcc 和构建工具，规则见
[docs/LOCK.md](docs/LOCK.md)。

### 使用 OCI 构建环境

```bash
# 拉取并缓存镜像（一次即可，跨项目共享）
phprt buildimages add quay.io/fedora/fedora:43 --name fedora43

# 用镜像里的工具编译
phprt install php@8.5 --image fedora43 --instance-name php-build
./myphp-8.5 -v
```

构建实例会保留已装工具，后续 `add` / `update` / `install` 沿用 state 里的选择。只支持
本地模式、Linux 同架构、gzip / 未压缩 tar 层；完整说明见
[docs/BUILD-IMAGES.md](docs/BUILD-IMAGES.md)。

### 全局安装

```bash
phprt --global --prefix /usr/local/php install php@8.5 --targets="cli"
/usr/local/php/bin/php -v
```

安装树就是 prefix，不生成项目入口；`install` / `add` / `remove` / `update` /
`scheme` / `plan` 的用法相同，只是每条命令都带 `--global --prefix <目录>`。

## 配置

默认读 `<root>/phprt.toml`（`--root` 默认当前目录），`-c/--config` 可以指定别的文件。
最常改的键：

| 键 | 默认 | 说明 |
| --- | --- | --- |
| `recipes` | `./recipes` | recipe 目录 |
| `download_dir` | 本地 `.phprt/downloads`，全局 `~/phprtdownloads` | 源码缓存 |
| `local_internal_dir` | `.phprt` | 状态、安装树、缓存、备份的根目录 |
| `parallel` | CPU 数 - 1（最少 1） | `make -j` / cmake 并行度 |

文件里每个键都有注释和默认值。并行度只从这里和 `--parallel` 取；shell 里的
`MAKEFLAGS` / `CMAKE_BUILD_PARALLEL_LEVEL` 不生效。完整参数见
[docs/CLI.md](docs/CLI.md)。

## 目录

项目模式：安装树 `.phprt/php-8.5.13/`，方案 `.phprt/php-8.5/state.json`，源码缓存
`.phprt/downloads/`，ini 备份 `.phprt/backup/`，解压目录 `build/`，入口 `myphp-8.5`
（选中 cli target 时生成）。

全局模式下安装树就是 `--prefix`；源码解压、下载、备份默认在 `~/phprtbuild`、
`~/phprtdownloads`、`~/phprtbackup`。各目录的配置项见 [docs/CLI.md](docs/CLI.md)，
状态字段见 [docs/STATE.md](docs/STATE.md)。

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/CLI.md](docs/CLI.md) | 完整命令、参数、错误和退出码 |
| [docs/FLOW.md](docs/FLOW.md) | install / add / remove / scheme 的流程与判定 |
| [docs/TARGETS.md](docs/TARGETS.md) | `--targets`、CLI / FPM 配置 |
| [docs/SOLVER.md](docs/SOLVER.md) | 选线、依赖线交集、依赖边、feature 并集 |
| [docs/BUILD.md](docs/BUILD.md) | 构建顺序、环境、签名、日志 |
| [docs/LOCK.md](docs/LOCK.md) | `--lock`、`scheme --lock`、补丁复用 |
| [docs/BUILD-IMAGES.md](docs/BUILD-IMAGES.md) | 用 OCI 镜像构建：缓存、实例、删除、运行库差异 |
| [docs/DESIGN.md](docs/DESIGN.md) | 设计与全部子文档入口 |
| [docs/plan/RUNTIME-IMAGE.md](docs/plan/RUNTIME-IMAGE.md) | 从安装树产出运行镜像（未实现的设计） |

## 许可

MIT，见 [LICENSE](LICENSE)。
