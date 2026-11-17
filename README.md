# phprtv3

用源码编译 PHP / nginx 的命令行工具。安装结果放在项目目录里，不污染系统。

## 1. 构建 phprt

```bash
cargo build --release
cp target/release/phprt ~/.local/bin/      # 或者直接用 ./target/release/phprt
```

## 2. 在项目里使用

`--recipes` 是 recipe 目录，默认当前目录下的 `./recipes`；`--root` 是项目根，默认 `.`，
安装树和状态都写在 `<root>/.phprt/` 下，例如项目根是 `/www/blog` 就是 `/www/blog/.phprt/`。
这些都可以写进项目根的 `phprt.toml`：`recipes`、`internal_dir`、`build_dir`、`backup_dir`；
命令行 `--recipes` / `--internal-dir` / `--build-dir` / `--backup-dir` 优先于配置，
目录可以写绝对路径（如 `--internal-dir /opt/php`）。

项目里没有 `recipes/` 时，要么在 `phprt.toml` 里配 `recipes = "..."`，要么显式传 `--recipes`：

```bash
cd ~/myproject
phprt --recipes /path/to/phprtv3/recipes --root . install php@8.5
```

建议在 `~/.bashrc` 里固定 recipe 路径，之后在项目目录直接用 `phprt`：

```bash
phprt() { command phprt --recipes /path/to/phprtv3/recipes "$@"; }
```

## 3. 第一次安装

```bash
phprt install php@8.5          # 装 PHP 8.5 和默认扩展
./myphp-8.5 -v                 # 项目自己的 PHP
./myphp-8.5 -m                 # 已加载的扩展
```

- 源码下载到 `.phprt/downloads/`，解压到 `build/`，装到 `.phprt/php-8.5.12/`；
- 项目里只有一个 PHP 系列时，另外生成 `./myphp`；
- 中途失败或中断，把同一条命令再跑一次会接着装。

## 4. 查看

```bash
phprt list                     # 已安装的产品
phprt list php@8.5             # 组件、支持的方式、版本、状态
phprt list php@8.5 --installed # 只看已装
phprt list php@8.5 --pending   # 只看已加入方案但还没装
phprt plan php@8.5             # 构建计划：依赖、顺序、参数
```

## 5. 加扩展

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

如果方案里有、磁盘上缺文件（比如 ini 被删了），`add` 会提示并拒绝；确认要重新编译时加 `--force`：

```bash
phprt add php@8.5 swoole --force
```

只下载源码到缓存（不改方案、不解压、不编译）：

```bash
phprt add php@8.5 swoole --downloads
```

## 6. 移除扩展

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

## 7. 重装、升级、降级

```bash
phprt update php@8.5            # 跟随 catalog 升到 8.5 线最新补丁
phprt install php@8.5.13        # 指定补丁
phprt install php@8.5.12        # 已装同一补丁：拒绝，并提示怎么重装
phprt install php@8.5.12 --force
```

`--force` 会把现有安装树改名成 `.phprt/php-8.5.12-backup-<日期>`，再全新安装。

## 8. 换台机器复现同一套方案

```text
$ phprt scheme php@8.5
phprt install php@8.5 'curl=mode:static,gd=mode:shared,swoole,xdebug=mode:shared'
```

把输出里的命令拿到另一台机器（或另一个项目目录）执行，就得到同一套组件和编译方式。
补丁号按当时的 catalog 取，不锁死。

反过来也可以直接给 `install` 一份完整列表：

```bash
phprt install php@8.5 'curl=mode:shared,gd=mode:shared,swoole,redis'
```

列表就是完整方案；会替换当前方案，不跟默认组件合并。

## 9. 装到项目外（/opt 之类）

默认装在项目里；想把 PHP 装到 `/usr/local/php` 这类位置时用全局模式：

```bash
phprt --global --prefix /usr/local/php install php@8.5
/usr/local/php/bin/php -v

phprt --global --prefix /usr/local/php add php@8.5 swoole
phprt --global --prefix /usr/local/php remove php@8.5 swoole
```

- 安装树、状态和完成标记都在 `<prefix>` 里，不生成项目入口；
- 源码默认解压到 `~/phprtbuild`，下载缓存默认 `~/phprtdownloads`，ini 备份默认 `~/phprtbackup`；
  下载目录用 `download_dir` / `--download-dir`，另外两个用 `global_build_dir` /
  `global_backup_dir` 或 `--build-dir` / `--backup-dir` 改；
- 重复安装、`--force`、add/remove 的判定和本地一致；`--force` 会把 prefix 改名成
  `<prefix>-backup-<日期>`。

## 10. 只下载、离线、代理

```bash
phprt install php@8.5 --downloads          # 只下载源码，不解压、不写状态
phprt install php@8.5 --offline            # 只用缓存
phprt install php@8.5 --proxy http://127.0.0.1:10808
phprt install php@8.5 --download-jobs 10 --download-host-jobs 2 --connections 4 --timeout 900
```

下载中断后重跑同一条命令会接着下；连接被重置会自动重试。

## 11. 只装 PHP 本体

```bash
phprt install php@8.5 \
  --disable-default-component openssl \
  --disable-default-component curl \
  --disable-default-component libxml
```

## 12. nginx

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

## 13. 目录

```text
.phprt/php-8.5.12/            安装树（目录名默认 .phprt，可配置）
.phprt/php-8.5/state.json     当前方案
.phprt/downloads/             源码缓存（跨产品共享）
.phprt/backup/                删除前的 ini 备份（backup_dir 可改）
.phprt/php-8.5.12-backup-*    install --force 留下的旧安装树
build/                        解压后的源码（build_dir 可改）
myphp-8.5                     项目入口
```

## 14. 构建工具

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

## 15. 可装的产品

```text
PHP    7.4 / 8.0 / 8.1 / 8.2 / 8.3 / 8.4 / 8.5 / 8.6
nginx  1.29
```

PHP 扩展用 `phprt list php@8.5` 查看，包含内置扩展（curl、gd、mbstring…）和 PECL 扩展
（swoole、xdebug、redis、imagick…），以及每个扩展支持的方式（static / shared）。

## 16. 更多

- [docs/CLI.md](docs/CLI.md)：完整命令、参数、错误
- [docs/BUILD.md](docs/BUILD.md)：构建、日志、跳过规则
- [docs/DESIGN.md](docs/DESIGN.md)：设计与各子文档入口
