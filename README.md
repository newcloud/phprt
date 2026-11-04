# phprt

按项目隔离的 PHP 工具链：仓库只做软件源，每个项目的 `.phprt/` 自带 runner、源码和编译树。
装完之后运行时不需要任何环境变量，靠 `$ORIGIN` rpath 找依赖库。

```bash
cd /path/to/project
/path/to/phprt/phprt set 8.5.12 --ext "openssl zlib curl sqlite redis" --yes
./myphp -v
```

## 支持

- `php-8.5.12`：PHP 官方源码包。
- `php-8.6.0-dev`：GitHub `PHP-8.6` 分支归档，需要 `buildconf`。
- `php-7.4.33`：PHP 官方源码包，目前只有最小档：`libxml2` + 内核基础扩展，
  其它内置扩展和 PECL 还没适配。
- 内置扩展：`openssl zlib libxml2 curl sqlite mysqli mbstring intl zip gd ffi`
- PECL 扩展：`redis protobuf xdebug apcu yaml swoole`

PHP 7.4 的 `ext/libxml` 没有 `config.m4`，libxml2 是内核强制依赖，所以 7.4 的
`--ext` 至少要包含 `libxml2`（固定 2.9.14，新版删了旧 API）。7.4 的 opcache 编译成
共享扩展，构建时会写 `<tree>/etc/conf.d/10-opcache.ini` 显式加载。

`sqlite` 编成共享模块，可以按项目启用/停用；其余内置扩展目前静态编进 PHP，PECL 扩展是独立 `.so`。

## 用法

```bash
cd /path/to/project

# 首次：复制 runner/bundle/源码并构建
/path/to/phprt/phprt set 8.5.12

# 指定扩展（逗号或空格分隔）
/path/to/phprt/phprt set 8.5.12 --ext "openssl zlib libxml2 curl sqlite mysqli mbstring zip redis"

# 只看计划，不构建
/path/to/phprt/phprt set 8.5.12 --ext "sqlite redis" --plan

# 强制重编 PHP，但依赖和已完成的扩展仍复用
/path/to/phprt/phprt set 8.5.12 --recompile

# 清源码和全部状态，从头来
/path/to/phprt/phprt set 8.5.12 --clean-src

phprt current
phprt list
phprt add ext yaml
phprt remove ext yaml
phprt verify
```

`add ext`：

- 已经编译好的扩展 → 只启用；
- 未编译的 PECL 扩展 → 只编这个扩展和它的依赖，PHP 不动；
- 未编译的内置扩展 → 需要重新 configure PHP，会提示并重编 PHP。

`remove ext` 只删项目 `conf.d` 里的 ini，不删 `.so`；静态内置扩展无法真正移除。

仓库存在时，`set` / `add ext` 会先把 runner 和对应 bundle 同步到项目，再构建；
项目里始终有一份可独立运行的副本。

`set` 之后可以只使用项目内的入口：

```bash
./.phprt/bin/phprt current
./myphp -v
./myphp --env

# 把同一套环境导进当前 shell，之后直接敲 php / composer
eval "$(./myphp --shell)"
```

## 运行时：不再需要 LD_LIBRARY_PATH

构建收尾会把 `deps/*/lib` 里的共享库整理到 `<tree>/lib/phprt/`：

- 名字加 8 位内容 hash（`libz.so.1` → `libz-523f183f.so.1`），避免和系统库或别的版本撞名；
- 库自己的 `SONAME` 同步改写，所有消费方的 `DT_NEEDED` 一起改；
- `bin/php`、各 `*.so`、`lib/phprt/*` 的 `RPATH` 统一写成 `$ORIGIN` 相对路径。

于是整棵树可以随便搬，`myphp` 也不用再拼 `LD_LIBRARY_PATH`。
名字映射记录在 `<tree>/lib/phprt/wheel.map`。OpenSSL 的 `openssl.cnf` 会复制到
`<tree>/etc/ssl/`，运行时同样不再依赖 `deps/`。

## 构建阶段

```text
resolve
  → deps（按 needs 拓扑排序）
  → php
  → ext（每个扩展独立判断）
```

每一步有自己的签名，成功后写到 `<tree>/.phprt-state/`：

- 依赖：recipe + 版本 + 传递依赖；
- PHP：最终 configure 行 + builtin recipe + PHP 用到的依赖签名；
- PECL：扩展 recipe + PHP ABI + 自己的依赖签名。

规则：

- 依赖不变 → 跳过；
- 只有 PECL 增加/删除 → 跳过 PHP，只处理扩展；
- 只有 PHP 配置或它用到的依赖变化 → 重编 PHP，PECL 按 ABI 判断；
- 源码已解压就复用，只有 `--clean-src` 才重新解压。

## 源码缓存与工具

正常使用不需要设置任何环境变量。

`set` 和 `add ext` 按以下顺序找源码包：

1. `.phprt/downloads/`
2. `PHPRT_SEED`（可选）
3. 仓库 `downloads/php-<系列>/`（例如 `downloads/php-7.4/`）
4. 仓库 `downloads/`
5. `~/.phprt/downloads/`

找不到时按 bundle 里的 URL 下载。源码包不进 git，仓库里只保留
`downloads/README.md` 和 `downloads/SHA256SUMS`。

8.6 的 `buildconf` 和 PECL 的 `phpize` 需要 autoconf、re2c 和 perl 核心模块。缺少时工具 recipe 会从源码构建到 `.phprt/tools/`，不需要手工准备。

## 目录

仓库：

```text
phprt/
├── phprt                  # 入口：在仓库里跑，或复制到项目 .phprt/bin/
├── runner/                # 通用执行器（复制到 <项目>/.phprt/bin/runner/）
├── bundles/
│   ├── php-7.4.33/
│   ├── php-8.5.12/
│   └── php-8.6.0-dev/
├── downloads/             # 源码种子缓存（tarball 不进 git）
├── docs/
└── LICENSE
```

项目：

```text
<项目>/
├── myphp
└── .phprt/
    ├── bin/phprt              # 复制来的本地入口
    ├── bin/runner/            # 通用执行器
    ├── bundles/php-<版本>/    # 复制来的版本脚本和 recipe
    ├── downloads/             # 选中源码
    ├── build/                 # 解压目录，可删
    ├── php-<版本>/            # 自包含树（deps 在树内）
    │   ├── lib/phprt/         # 运行时库（hash 名 + $ORIGIN rpath）
    │   ├── deps/
    │   ├── etc/ssl/           # openssl.cnf
    │   ├── .phprt-state/      # 每步签名
    │   ├── BUILD-PLAN
    │   ├── BUILD-ORDER
    │   ├── BUILD-STATUS       # building / ok
    │   └── .build-exts        # 本次选择
    ├── logs/
    ├── conf.d/                # 项目启用的扩展
    ├── current -> php-<版本>
    └── project.conf
```

## 演示

```bash
mkdir -p /tmp/phprt-demo && cd /tmp/phprt-demo

/path/to/phprt/phprt set 8.5.12 \
  --ext "openssl zlib libxml2 curl sqlite mysqli mbstring zip" --yes

./myphp -v
./myphp -m | grep -E 'sqlite|curl|mbstring'

# 再跑一次：依赖、PHP、扩展全部跳过
/path/to/phprt/phprt set 8.5.12 \
  --ext "openssl zlib libxml2 curl sqlite mysqli mbstring zip" --yes

# 加一个 PECL 扩展：只编它，不碰 PHP
/path/to/phprt/phprt add ext yaml
```

## 从 v1 迁移

v1 是全局安装根模型（`install.sh` + `~/.phprt`，仓库里 `lib/modules` 放 recipe）。
v2 改成项目自包含，两者不兼容：

- 不需要 `install.sh`，也没有全局安装根；
- 旧项目的 `.phprt` 不能复用，重新 `phprt set <版本>` 即可（源码缓存会复用 `downloads/`）；
- 旧版本代码留在 `v1-legacy` 标签上：`git checkout v1-legacy`。

## 限制

- 内置扩展多数是静态编入，`remove ext` 只能从项目 ini 移除，不能卸载。
- `add ext` 加未编译的内置扩展会重编 PHP。
- `--clean-src` 会清掉源码和全部签名，下一次是完整构建。
- 没有 bundle 刷新、旧依赖清理和 7.4 适配。
- `verify` 检查 PHP/扩展的 `ldd`、RPATH 和实际加载，不检查 bundle 完整性。
