# phprtv3

用源码编译 PHP / nginx 的命令行工具。默认把结果装到当前项目目录里，也可以用全局模式装到
`/usr/local/php` 这类位置。

## 构建 phprt

```bash
cargo build --release
cp target/release/phprt ~/.local/bin/      # 或者直接用 ./target/release/phprt
```

## 项目模式（默认）

### phprt.toml

默认读 `<root>/phprt.toml`（`--root` 默认当前目录）；用 `-c/--config` 可以指定别的文件：

```bash
phprt -c ~/myphprt.toml install php@8.5
```

常用键（文件里每个键都有注释和默认值，直接看 `phprt.toml`）：

| 键 | 默认 | 说明 |
| --- | --- | --- |
| `recipes` | `./recipes` | recipe 目录 |
| `download_dir` | 本地 `local_internal_dir/downloads`，全局 `~/phprtdownloads` | 下载缓存 |
| `local_internal_dir` | `.phprt` | 状态、安装树、缓存、备份的根目录 |
| `local_build_dir` | `build` | 源码解压目录 |
| `local_backup_dir` | `local_internal_dir/backup` | ini 备份目录 |
| `global_build_dir` | `~/phprtbuild` | 全局模式的解压目录 |
| `global_backup_dir` | `~/phprtbackup` | 全局模式的备份目录 |

本地模式的相对路径按项目根解析；命令行参数优先于配置。

### 在项目里使用

`--recipes` 是 recipe 目录，默认 `./recipes`；`--root` 是项目根，默认 `.`。
项目里没有 `recipes/` 时，在 `phprt.toml` 里配 `recipes = "..."`，或者显式传 `--recipes`：

```bash
cd ~/myproject
phprt --recipes /path/to/phprtv3/recipes --root . install php@8.5
```

建议在 `~/.bashrc` 里固定 recipe 路径，之后在项目目录直接用 `phprt`：

```bash
phprt() { command phprt --recipes /path/to/phprtv3/recipes "$@"; }
```

### 第一次安装

```bash
phprt install php@8.5          # 装 PHP 8.5 和默认扩展
./myphp-8.5 -v                 # 项目自己的 PHP
./myphp-8.5 -m                 # 已加载的扩展
```

- 源码下载到 `.phprt/downloads/`，解压到 `build/`，装到 `.phprt/php-8.5.12/`；
- 项目里只有一个 PHP 系列时，另外生成 `./myphp`；
- 中途失败或中断，把同一条命令再跑一次会接着装。

### 查看

```bash
phprt list                     # 已安装的产品
phprt list php@8.5             # 组件、支持的方式、版本、状态
phprt list php@8.5 --installed # 只看已装
phprt list php@8.5 --pending   # 只看已加入方案但还没装
phprt plan php@8.5             # 构建计划：依赖、顺序、参数
```

### 加扩展

`add` 会改方案并立刻编译，不需要再跑 `install`：

```bash
phprt add php@8.5 swoole
./myphp-8.5 -m | grep swoole
```

内置扩展（在 PHP 源码里的）必须写明编译方式：

```bash
phprt add php@8.5 'gd=mode:shared'   # 编成动态扩展，不重编 PHP
phprt add php@8.5 'gd=mode:static'   # 编进 PHP，会重编（几分钟）
```

不写方式会直接报错并提示这两条命令。

带参数的选择：

```bash
phprt add php@8.5 'swoole=version:6.0'
phprt add php@8.5 'curl=mode:shared,dep:libcurl[ssl,nghttp2]'
phprt add php@8.5 'gd=mode:shared,feature:[jpeg,webp]'
phprt add php@8.5 redis
```

- `dep:名字[feature,...]`：打开组件对依赖的集成，并把 feature 转给依赖；
- `feature:[...]`：组件自己的编译开关；`feature:[]` 表示全部关闭；
- 加多个扩展就执行多条 `add`。

如果方案里有、磁盘上缺文件（比如 ini 被删了），`add` 会提示并拒绝；确认要重新编译时加
`--force`：

```bash
phprt add php@8.5 swoole --force
```

只下载源码到缓存（不改方案、不解压、不编译）：

```bash
phprt add php@8.5 swoole --downloads
```

### 移除扩展

```bash
phprt remove php@8.5 swoole
```

- 动态扩展：删掉 `*.ini` 和 `.so`，不重编；
- nginx 动态模块：和 nginx 一起重编，删掉 `modules/*.so`（含 `.old`）；
- 静态扩展：会重编 PHP，执行前会提示耗时，建议先自行备份安装树：
  ```bash
  cp -a .phprt/php-8.5.12 .phprt/php-8.5.12.bak-$(date +%F-%H%M)
  ```
- 删除 ini 前会自动备份到 `.phprt/backup/<日期时间>_<组件>.ini`；
- ini 被手工改过、或 ini/`.so` 缺失时会拒绝；确认要执行加 `--force`：
  ```bash
  phprt remove php@8.5 swoole --force
  ```
- 不需要这份 ini 备份时加 `--no-backup-php-ext-ini`：
  ```bash
  phprt remove php@8.5 swoole --no-backup-php-ext-ini
  ```

### 重装、升级、降级

```bash
phprt update php@8.5            # 跟随 catalog 升到 8.5 线最新补丁
phprt install php@8.5.13        # 指定补丁
phprt install php@8.5.12        # 已装同一补丁：拒绝，并提示怎么重装
phprt install php@8.5.12 --force
```

`--force` 会把现有安装树改名成 `.phprt/php-8.5.12-backup-<日期>`，再全新安装。

### 换台机器复现同一套方案

```text
$ phprt scheme php@8.5
phprt install php@8.5 'curl=mode:static,gd=mode:shared,swoole,xdebug=mode:shared'
```

把输出里的命令拿到另一台机器（或另一个项目目录）执行，就得到同一套组件和编译方式。
补丁号按当时的 catalog 取，不锁死。

反过来也可以直接给 `install` 一份完整列表（替换当前方案）：

```bash
phprt install php@8.5 'curl=mode:shared,gd=mode:shared,swoole,redis'
```

### 只下载、离线、代理

```bash
phprt install php@8.5 --downloads          # 只下载源码，不解压、不写状态
phprt install php@8.5 --offline            # 只用缓存
phprt install php@8.5 --proxy http://127.0.0.1:10808
phprt install php@8.5 --download-jobs 10 --download-host-jobs 2 --connections 4 --timeout 900
```

下载中断后重跑同一条命令会接着下；连接被重置会自动重试。

### 只装 PHP 本体

```bash
phprt install php@8.5 \
  --disable-default-component openssl \
  --disable-default-component curl \
  --disable-default-component libxml
```

## 全局模式（--global --prefix）

把产品直接装到项目外的目录，比如 `/opt`、`/usr/local/php`：

```bash
phprt --global --prefix /usr/local/php install php@8.5
/usr/local/php/bin/php -v

phprt --global --prefix /usr/local/php add    php@8.5 swoole
phprt --global --prefix /usr/local/php remove php@8.5 swoole
phprt --global --prefix /usr/local/php list   php@8.5
phprt --global --prefix /usr/local/php scheme php@8.5
```

和项目模式的区别只有这几点：

- 安装树就是 `--prefix`；状态和完成标记在 `<prefix>/.phprt-state.json`、`<prefix>/.phprt-installed`；
- 不生成项目入口，直接用 `<prefix>/bin/php`；
- 解压、下载、备份默认在 `~/phprtbuild`、`~/phprtdownloads`、`~/phprtbackup`；
  用 `global_build_dir`、`global_backup_dir`、`download_dir` 或对应的 `--build-dir` /
  `--backup-dir` / `--download-dir` 改；
- 重复安装、`--force`、add/remove、`scheme`、`plan`、下载/离线这些用法和项目模式完全一样，
  只是每条命令都要带上同样的 `--global --prefix <目录>`；
- `--force` 会把 prefix 改名成 `<prefix>-backup-<日期>`。

## nginx

```bash
phprt install nginx@1.29
.phprt/nginx-1.29.8/sbin/nginx -V

phprt add nginx@1.29 lua-nginx-module
ls .phprt/nginx-1.29.8/modules/
```

nginx 动态模块和 nginx 一起构建：`add` / `remove` 会重新 configure/make（约 1 分钟），
执行前会提示并给出备份命令；非交互环境（脚本、CI）加 `--force` 确认。

```bash
phprt add nginx@1.29 lua-nginx-module --force
phprt remove nginx@1.29 lua-nginx-module --force
```

nginx 不生成 `myphp` 之类的入口，直接用安装树里的二进制。

## 目录

项目模式（默认）：

```text
.phprt/php-8.5.12/            安装树（local_internal_dir 可改）
.phprt/php-8.5/state.json     当前方案
.phprt/downloads/             源码缓存（download_dir 可改）
.phprt/backup/                删除前的 ini 备份（local_backup_dir 可改）
.phprt/php-8.5.12-backup-*    install --force 留下的旧安装树
build/                        解压后的源码（local_build_dir 可改）
myphp-8.5                     项目入口
```

全局模式：

```text
/usr/local/php/                       安装树（--prefix）
/usr/local/php/.phprt-state.json      方案
/usr/local/php/.phprt-installed       完成标记
/usr/local/php/bin/php                产品入口
~/phprtbuild/                         解压后的源码
~/phprtdownloads/                     源码缓存
~/phprtbackup/                        ini 备份
```

## 构建工具

编译前检查宿主机命令；下列是产品至少需要的：

| 产品 | 命令 |
| --- | --- |
| php | `gcc` ≥ 9、`g++` ≥ 9、`make` ≥ 4.0、`pkg-config` ≥ 0.29、`perl` ≥ 5.20、`m4` ≥ 1.4、`patchelf` ≥ 0.13、`ar`、`ranlib`、`sh` |
| nginx | `gcc` ≥ 8、`make` ≥ 4.0、`perl` ≥ 5.20、`patchelf` ≥ 0.13、`ar`、`ranlib`、`sh` |

个别依赖还会用到 `cmake`、`nasm` 等，缺了会在下载前报出来：

```text
error: 需要 gcc >= 9，当前 1.0.0
error: 缺少构建工具: patchelf
```

## 可装的产品

```text
PHP    7.4 / 8.0 / 8.1 / 8.2 / 8.3 / 8.4 / 8.5 / 8.6
nginx  1.29
```

PHP 扩展用 `phprt list php@8.5` 查看，包含内置扩展（curl、gd、mbstring…）和 PECL 扩展
（swoole、xdebug、redis、imagick…），以及每个扩展支持的方式（static / shared）。

## 更多

- [docs/CLI.md](docs/CLI.md)：完整命令、参数、错误
- [docs/BUILD.md](docs/BUILD.md)：构建、日志、跳过规则
- [docs/DESIGN.md](docs/DESIGN.md)：设计与各子文档入口
