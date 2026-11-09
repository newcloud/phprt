# phprtv2 开发者文档

本文写给要改 phprt 本身、加扩展或排查构建问题的人。普通用户看
[README](../README.md)。

## 1. 定位

phprt 分成两层：

- `bundles/php<系列>/**` 是**数据**：版本、下载地址、依赖关系、编译参数、ini 内容。
- `runner/*.sh` 是**驱动**：解析数据、算依赖拓扑、管签名/锁/安装树，不出现具体包名。

验收标准只有一条：

> 加一个扩展或依赖，只改 `bundles/`，`runner/` 零改动，`phprt verify` 仍然全绿。

仓库只做软件源。装好的东西在项目自己的 `.phprt/` 里，不在仓库里生成运行树。

## 2. 数据与驱动解耦

### 2.1 目录

```text
bundles/php8.5/
├── bundle.conf          # PHP 版本、全局策略、所有依赖的版本和下载地址
├── php.sh               # PHP 本体的 configure / build
├── builtin/<名>.sh      # PHP 内置扩展（编进 PHP 或独立 .so）
├── deps/<库>.sh         # 第三方库
├── ext/<名>.sh          # PECL 扩展
└── tools/<工具>.sh      # 构建工具（autoconf、re2c、perlshim）
```

### 2.2 recipe 契约

每个 recipe 第二行是契约：

```bash
# phprt: contract=v2 kind=dep name=openssl version=OPENSSL_VERSION needs= optional= tools=perlshim wheel=singleton:libssl+libcrypto upstream=github:openssl/openssl
```

驱动只通过这行字段和标准函数取信息：

| 字段 | 作用 |
| --- | --- |
| `kind` | `builtin` / `dep` / `ext` / `tool` |
| `name` | 模块名 |
| `version` | 版本变量名（值在 `bundle.conf`） |
| `switch` | 对应的 `ENABLE_*` 开关 |
| `modules` | 扩展提供的 PHP 模块名 |
| `ini` | 共享扩展的 ini 文件名 |
| `needs` | 硬依赖 |
| `optional` | 可选依赖 |
| `tools` | 构建工具 |
| `wheel` | 运行时库例外声明 |
| `upstream` | 上游版本来源 |

函数：

- `build()`：构建并安装
- `flags()`：给 PHP configure 的参数（builtin）
- `needs()`：动态依赖
- `enable()`：写 ini（ext）
- `versions()` / `latest_version()`：特殊上游来源

完整规范见 [RECIPE.md](RECIPE.md)。

### 2.3 解耦的边界

- `runner/` 不判断“这是 openssl 还是 libpng”，只按 `kind` 和签名处理。
- 条件依赖由 recipe 的 `needs()` 在子进程里动态给出。
- 同名模块（如 `curl` 既是 builtin 又是 dep）由 `module_sh` 和
  `ext_recipe_sh` 分开解析：依赖走 `deps/` 优先，扩展走 `builtin/ext`。
- 新增 recipe 后必须跑：

```bash
scripts/recipe-check.sh           # 静态检查
scripts/recipe-check.sh --deep    # 再联网确认上游版本
```

## 3. 目录与生命周期

### 3.1 仓库

```text
<repo>/
├── phprt                 # 入口
├── runner/               # 驱动
├── bundles/php8.5/       # 数据
├── scripts/              # recipe-check.sh、buildroot.sh
├── downloads/            # 源码包种子（本地缓存，不进 git）
└── docs/
```

### 3.2 项目

```text
<project>/
├── myphp
└── .phprt/
    ├── current -> php-8.5.12
    ├── php-8.5.12/            # 标准 PHP 安装树
    │   ├── bin/php
    │   ├── etc/{php.ini,conf.d,ssl/cert.pem}
    │   ├── lib/phprt/         # 运行时库 + wheel.map
    │   └── deps/<库>-<版本>/
    ├── build-php-8.5.12/      # 构建区，可删
    │   ├── src/ logs/ tools/ downloads/
    │   └── .lock
    └── buildroots/            # 可选，chroot rootfs
```

约束：

- `.phprt` 顶层只放 `current`、`php-*`、`build-php-*`、`buildroots`。
- 构建区按 PHP 版本分，删掉只影响该版本的增量重建。
- 扩展状态不落盘，按 `myphp` 相同环境跑 `php -m` 推导。

### 3.3 构建基目录

`PHPRT_WORK` 可以把构建区放到项目外：

```text
<PHPRT_WORK>/build-php-<版本>/
```

自动 chroot 构建时，项目外的构建区会按原绝对路径额外绑定。

## 4. 构建流程

### 4.1 入口

`phprt install` 和 `phprt add ext` 最终都调用：

```bash
runner/build.sh --bundle <bundle> --tree <tree> --exts "<扩展列表>"
```

`build.sh` 做这些事：

1. 读扩展选择，按 `builtin` / `ext` 分开；
2. 按 `needs()` 递归算出依赖闭包和拓扑顺序；
3. 逐个构建依赖；
4. 构建 PHP；
5. 构建 shared 扩展；
6. 整理运行时库（wheel）；
7. 写 `BUILD-INFO`、`BUILD-STATUS=ok`。

`--plan` 只打印依赖、PHP、static/shared 分组，不创建目录，也不进 chroot。

### 4.2 签名与增量

每个阶段都有签名，签名不变就跳过：

| 阶段 | 签名内容 |
| --- | --- |
| `deps/<库>` | recipe hash、版本、LTO、依赖的 ABI 身份和安装目录 |
| `php` | configure 参数、PHP 版本、链接方式、LTO、static builtin、static PECL、依赖身份 |
| `ext/<扩展>` | recipe hash、版本、模式、PHP ABI、依赖身份 |

签名文件在 `$TREE/.phprt-state/`；依赖自己的 stamp 在依赖安装目录里。

注意：依赖安装目录名（含版本）会进 PHP 和扩展的签名。这是故意的：
autotools 的 `.la` 里记的是绝对路径，依赖目录被清理后旧 `.la` 失效，消费方
必须重编。所以同系列升级依赖时，PHP 或扩展可能会跟着重编。

### 4.3 `phprt update`

`phprt update <依赖> [版本]` 只处理依赖库：

1. 从当前树 `deps/<库>-*` 读实际版本；
2. 不写版本时，用 `outdated` 同一套上游查询，取同系列最新稳定补丁版；
3. 只允许 `major.minor` 不变、版本变高；
4. 改仓库 `bundle.conf` 的版本、URL、SHA256；
5. 下载新源码包并计算 sha256；
6. 按当前扩展选择重跑 `build.sh`；
7. 失败时回滚 `bundle.conf`、旧依赖目录、`lib/phprt`、`.phprt-state` 和
   `BUILD-STATUS`。

限制：

- URL 里必须能找到旧版本号，否则不能自动改；SQLite 的
  `sqlite-amalgamation-3540100.zip` 就是例外。
- 不升级 PHP 本体；PHP 小版本用 `phprt install 8.5.13 --ext "..."`。
- `--dry-run` 只打印计划，不下载、不构建。

## 5. 运行时库 wheel

依赖装好后，`wheelize_tree` 做这些事：

1. 读每个真实 `.so` 的 `SONAME`，生成稳定的运行时名：
   `libssl.so.3` → `libssl-rt3.so.3`；
2. 把库复制到 `$TREE/lib/phprt/`；
3. 用 `patchelf` 改库自己的 `SONAME`；
4. 改所有消费方（`bin/php`、扩展 `.so`、其它库）的 `DT_NEEDED`；
5. `RPATH` 写成 `$ORIGIN` 相对路径；
6. 把映射和 sha256 写进 `wheel.map`。

结果：

- 同名库不会和系统里的抢；
- 运行时不需要 `LD_LIBRARY_PATH`；
- 整棵树搬走或改名都能跑；
- 同系列小版本升级时 SONAME 不变，运行时名不变，消费方 `DT_NEEDED` 零改写。

`phprt verify` 会检查 wheel 的 SONAME、sha256、singleton 组和残留原名。
细节见 [WHEEL-LIBS.md](WHEEL-LIBS.md)。

## 6. 配置、CA 与 myphp

- 主配置：`<tree>/etc/php.ini`
- 扫描目录：`<tree>/etc/conf.d/`
- CA：`<tree>/etc/ssl/cert.pem`

约定：

- `.phprt` 根下不放 `php.ini` 和 `conf.d`。
- `php.ini` 里不写绝对路径；需要路径时用 `myphp` 导出的环境变量。
- CA 只有一份；OpenSSL 用 `SSL_CERT_FILE`，PHP 的 openssl / curl 用
  `conf.d/00-ssl.ini` 里的 `${SSL_CERT_FILE}`。
- 工具链默认值写 `00-ssl.ini`，用户覆盖写编号更大的 ini。
- `myphp` 启动时按自身位置算出版本树路径，再设置 `PHPRC`、
  `PHP_INI_SCAN_DIR`、`extension_dir` 等变量。

版本树必须可移动：整体改名或搬到别处后，`./myphp` 照常可用，不重编、不改配置。

## 7. 全静态相关

- `--static-all`：扩展和第三方依赖都链接进 `bin/php`，glibc 仍动态。
  当前支持 `openssl zlib curl sqlite mbstring`。见 [STATIC-ALL.md](STATIC-ALL.md)。
- `--static-all-lto`：在 `--static-all` 基础上给 PHP 和依赖都开 LTO。
  见 [STATIC-ALL-LTO.md](STATIC-ALL-LTO.md)。
- 全静态 libc 的分析和发布限制见 [STATIC-LIBC.md](STATIC-LIBC.md)。

静态树不能再 `add ext --shared`，因为不存在可用的依赖 `.so` 链接路径。

## 8. chroot 构建（buildroot）

完整实现细节和环境变量清单见 [CHROOT-BUILD.md](CHROOT-BUILD.md)；这里只列
结构和约定。

### 8.1 布局

```text
<project>/.phprt/buildroots/
├── default/          # dnf --installroot 的 rootfs
├── .config           # ROOT_DEFAULT / BUILD_ENV=chroot|host
├── .lock             # buildroot 操作互斥
└── .home/            # bind 到 chroot 内，作为构建用户 HOME
```

`create` 支持简单名字（`default`、`ci`）和带 `/` 的任意路径。配置和锁都放在
`buildroots/` 里，不增加 `.phprt` 顶层文件。

### 8.2 挂载

`enter` / `exec` 时挂载：

- `/proc`
- `/sys`（失败时继续）
- 最小 `/dev`：tmpfs + 独立 devpts + `null/zero/full/random/urandom/tty`
- `/dev/shm`
- 默认 bind：项目根、phprt 仓库
- 自动构建额外 bind：`BUILD_ROOT`、`PHPRT_SEED`、`DOWNLOADS`

bind 必须保持绝对路径不变，否则 configure 的 `--prefix` 和
`--with-*=绝对路径` 会失效。

### 8.3 项目内 rootfs 的递归可见

rootfs 在 `<project>/.phprt/buildroots/default` 时，直接 bind 项目根会让
chroot 内通过项目目录再次看到 rootfs。当前实现不额外叠 tmpfs：

- 构建只使用精确路径（`PHPRT_PROJECT`、`PHPRT_HOME`、`BUILD_ROOT`），
  不做递归扫描；
- 如果在项目 bind 上盖住 `buildroots`，会把宿主自己的
  `.phprt/buildroots` 也一起藏掉，反而影响自动构建；
- `.home` 单独 bind 到 `/home/phprt`，不经过项目目录。

### 8.4 执行

- 用户命令：`phprt buildroot create/enter/exec/unmount/remove/status/list/use/env`
- 自动构建：`phprt install --chroot`、`phprt add ext --chroot` 统一走
  `runner/common.sh:run_build_env`
- `run_build_env` 调 `scripts/buildroot.sh exec <rootfs> -- <命令>`
- chroot 内用 `--userspec=<宿主uid:gid>`，产物属主正确，不做构建后 `chown`
- 环境变量用显式白名单传入，不依赖 `sudo -E`
- 构建锁 fd 用 `9>&-` 关闭，避免锁被带进 chroot
- `trap` 在退出、Ctrl+C 时卸载

### 8.5 安全约束

- rootfs 必须有 `.phprt-buildroot` 标记才允许 `remove`
- 拒绝 `/`、`$HOME`、符号链接、phprt 仓库目录
- `remove` 先卸载，确认 0 挂载，再 `rm -rf --one-file-system`
- 有进程占用时默认拒绝，`--force` 才跳过

用户用法见 [README](../README.md) 的“用 chroot 构建”。

## 9. 新增扩展或依赖

1. 在 `bundles/php8.5/{builtin,ext}/` 或 `deps/` 加 recipe；
2. 版本、URL、SHA256 写进 `bundle.conf`；
3. 跑 `scripts/recipe-check.sh`，通过后再跑 `--deep`；
4. 在测试项目里按 [EXTENSIONS.md](EXTENSIONS.md) 的流程做两轮验证；
5. 更新 [EXTENSIONS.md](EXTENSIONS.md) 的进度和台账。

driver 里不能出现具体包名。需要新能力时先改 recipe 字段或通用函数，再改
`runner/`。

## 10. 测试与验收

最低验收：

```bash
bash -n runner/*.sh scripts/*.sh
scripts/recipe-check.sh
phprt verify
```

`phprt verify` 检查七项：

1. PHP 能启动
2. 扩展能加载
3. 动态库只来自安装根
4. RPATH 只有 `$ORIGIN` 相对路径
5. php.ini 和 conf.d 在版本目录
6. CA 只有一份，openssl / curl 指向同一份
7. wheel 完整性

另外要做：

- 版本树改名/搬移后 `./myphp -v` 仍可用；
- 删除 `build-php-<版本>/` 后能重新增量构建；
- `--chroot` 构建后宿主 `phprt verify` 全绿，退出后挂载为 0；
- `phprt update <依赖>` 的 dry-run、无更新、跨系列拒绝、失败回滚。

实测记录在 [TEST-V2.md](TEST-V2.md)。

## 11. 项目约定

1. 一棵版本树就是标准 PHP 安装：`bin/etc/lib/deps`。
2. 配置只用 PHP 官方路径：`etc/php.ini`、`etc/conf.d/`、`etc/ssl/cert.pem`。
3. `.phprt` 根下不放 `php.ini` 和 `conf.d`。
4. 版本树必须可移动。
5. 产物不依赖构建树的绝对路径。
6. 会写死路径的库单独处理（如 OpenSSL 的 `--openssldir`）。
7. 运行时路径由 `myphp` 按自身位置算出。
8. 需要环境变量表达的配置只写在 `myphp`。
9. CA 只有一份，不引入第二份证书目录。
10. `php.ini` 不写绝对路径。
11. 工具链默认值和用户覆盖分开写 ini。
12. 用户只需要理解“项目里有一个标准 PHP 安装”。
13. 扩展状态不落盘，按 `php -m` 推导。
14. 构建目录按版本分。
15. 契约检查是开发门禁，只放在 `scripts/recipe-check.sh`。
16. 扩展进度、约束和问题台账只记在 [EXTENSIONS.md](EXTENSIONS.md)。
17. 开发是发现驱动的：需要改 driver 或流程时先停下报告，不擅自实施。

## 12. 已知限制

- 只接入 PHP 8.5。
- `phprt update` 只支持同系列依赖更新，且 URL 里必须含旧版本号。
- `install` / `add` 还没有自动在线检查；用 `phprt outdated` 手动看。
- `--static-all` 只覆盖 5 个扩展。
- chroot 构建依赖 dnf 系发行版。
- 只在 Fedora 43 上实测，未用于生产。

路线和后续任务见 [ROADMAP-V2.md](ROADMAP-V2.md)。
