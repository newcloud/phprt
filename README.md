# phprtv3

用源码编译 PHP / nginx 的命令行工具。默认把结果装到当前项目目录里，也可以用全局模式
装到 `/usr/local/php` 这类位置。

## 快速开始

```bash
cargo build --release
cp target/release/phprt ~/.local/bin/      # 或者直接用 ./target/release/phprt

phprt install php@8.5
./myphp-8.5 -v
./myphp-8.5 -m                             # 已加载的扩展
```

- 需要 Linux、Rust（只用来编 phprt），以及 gcc / g++ / make / pkg-config / perl /
  m4 / patchelf。缺命令时 phprt 会自己构建 catalog 里有的工具，其余按发行版给出
  安装命令。
- 源码下载到 `.phprt/downloads/`，解压到 `build/`，装到 `.phprt/php-8.5.13/`。
- 中途失败或中断，把同一条命令再跑一次会接着装。

## 常用命令

| 目的 | 命令 |
| --- | --- |
| 安装 | `phprt install php@8.5` |
| 加扩展 | `phprt add php@8.5 swoole` |
| 移除扩展 | `phprt remove php@8.5 swoole` |
| 升级补丁 | `phprt update php@8.5` |
| 查看组件 | `phprt list php@8.5` |
| 只看构建计划 | `phprt plan php@8.5` |
| 导出重建命令 | `phprt scheme php@8.5` |

```bash
phprt install php@8.5 --targets="cli,fpm"       # 同时装 FPM
phprt add php@8.5 'gd=mode:shared'             # 内置扩展必须写方式
phprt add php@8.5 'curl=mode:shared,dep:libcurl[ssl]'   # 打开组件对依赖的集成
phprt scheme php@8.5 --lock                     # 连补丁一起复现
```

- `add` 会改方案并立即编译；该系列还没装过时只记方案，提示先 `install`。
- `--targets` 会写进 state，之后不用重复传；改目标或配置后重装需要 `--force`。
- 加多个扩展就执行多条 `add`。组件写法见 [docs/CLI.md](docs/CLI.md)，
  判定和备份规则见 [docs/FLOW.md](docs/FLOW.md)。

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

## 两种模式

- **项目模式（默认）**：状态和安装树在 `.phprt/`，源码在 `build/`。PHP 选中 `cli`
  target 时生成项目入口 `myphp-<系列>`；只有一个 PHP 系列时还生成 `myphp`。
- **全局模式**：`phprt --global --prefix /usr/local/php install php@8.5 --targets="cli"`。
  安装树就是 prefix，不生成项目入口；`install` / `add` / `remove` / `update` /
  `scheme` / `plan` 的用法相同，只是每条命令都带 `--global --prefix <目录>`。

## 目录

项目模式：

```text
.phprt/php-8.5.13/         安装树
.phprt/php-8.5/state.json  当前方案
.phprt/downloads/          源码缓存
.phprt/backup/             删除前的 ini 备份
build/                     解压后的源码
myphp-8.5                  项目入口（选中 cli target 时生成）
```

全局模式下安装树就是 `--prefix`；源码解压、下载、备份默认在 `~/phprtbuild`、
`~/phprtdownloads`、`~/phprtbackup`。各目录的配置项见 [docs/CLI.md](docs/CLI.md)，
状态字段见 [docs/STATE.md](docs/STATE.md)。

## 其他产品

```bash
phprt install nginx@1.29
.phprt/nginx-1.29.8/sbin/nginx -V
```

当前可装 PHP 7.4 / 8.0 ~ 8.6 和 nginx 1.29；PHP 8.6 用 8.6.0RC3 构建。
组件清单用 `phprt list php@8.5` 查看。nginx 不生成入口，直接用安装树里的二进制。

## 用镜像构建

不想在宿主机装 gcc / make / patchelf 时，可以用 OCI 镜像里的工具编译：

```bash
phprt buildimages add quay.io/fedora/fedora:43 --name fedora43
phprt install php@8.5 --image fedora43 --instance-name php-build
./myphp-8.5 -v
```

之后的 `add` / `update` / `install` 沿用 state 里的选择。只支持本地模式、Linux
同架构、gzip / 未压缩 tar 层；完整说明见 [docs/BUILD-IMAGES.md](docs/BUILD-IMAGES.md)。

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/CLI.md](docs/CLI.md) | 完整命令、参数、错误和退出码 |
| [docs/FLOW.md](docs/FLOW.md) | install / add / remove / scheme 的流程与判定 |
| [docs/TARGETS.md](docs/TARGETS.md) | `--targets`、CLI / FPM 配置 |
| [docs/BUILD.md](docs/BUILD.md) | 构建顺序、环境、签名、日志 |
| [docs/BUILD-IMAGES.md](docs/BUILD-IMAGES.md) | 用 OCI 镜像构建：缓存、实例、删除、运行库差异 |
| [docs/DESIGN.md](docs/DESIGN.md) | 设计与全部子文档入口 |
| [docs/plan/RUNTIME-IMAGE.md](docs/plan/RUNTIME-IMAGE.md) | 从安装树产出运行镜像（未实现的设计） |

## 许可

MIT，见 [LICENSE](LICENSE)。
