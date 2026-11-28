# PHP 8.6 扩展支持

本文记录 PHP 8.6.0 正式版的扩展支持矩阵、catalog 选择规则和本地验收结果。扩展矩阵来自 2026-11-28 对 `php-8.6.0` 源码的实测；「可编译」只表示 `phpize` + `make` 通过，不代表运行时功能全部验收。

## 1. 正式版与 catalog

PHP 8.6.0 于 2026-11-19 发布：

```text
https://www.php.net/distributions/php-8.6.0.tar.gz
sha256 = 62337dccde3d9fcc39797d4529f42637613036eaf3875353c206874e5813a3c1
```

`recipes/catalog.toml` 的 `8.6` 线只有 `8.6.0`，默认补丁取数组内最大版本。

版本比较器把预发布后缀（`alpha` / `beta` / `rc` / `dev` / `pre`）排在无后缀正式版之前，例如 `2.0.0-beta1 < 2.0.0`。`1.1.1w` 这类补丁字母仍按字符串比较，规则见 [CATALOG-UPDATE.md](CATALOG-UPDATE.md) 6.3。

## 2. PHP 8.6 recipe

按 `php-8.6.0` 的 `ext/`、`config*.m4` 和 `configure --help` 核对：

- `BUILTIN_EXTS` 正确：`ext/imap` 只剩 `tests`，没有 `config.m4`；`ext/skeleton` 是扩展模板，不参与构建。
- `BUILTIN_DISABLE` 的 `--without-*` / `--disable-*` 开关全部仍然有效。
- `TARGETS` 的 CLI / CGI / FPM 开关全部仍然有效；`--disable-fpm` 仍然不存在。
- 产品 `configure` 的 `--without-pear`、`--enable-filter`、`--enable-phar`、`--enable-fileinfo`、`--enable-bcmath`、`--enable-calendar`、`--enable-exif` 均有效。
- gd 的 `tiff` / `heif` / `imagequant` / `uhdr` 四个 8.6 feature 与正式版 `ext/gd/config.m4` 一致。

recipe 的功能内容不需要按正式版调整，注释与文档中的版本号已同步为 `php-8.6.0`。

## 3. 扩展矩阵

实测方法：用 `php-8.6.0` 正式版源码构建最小 CLI，安装头文件和 `phpize`，逐个执行 `phpize`、`configure`、`make`。外部依赖扩展只做到编译阶段。

| 扩展 | 最新版本 | 结果 | 处理 |
| --- | --- | --- | --- |
| ssh2 | 1.6.1 | 编译通过，模块可加载 | 已加入 `1.6` 线，并写入 8.6 bundle |
| redis | 6.4.0RC1 | 6.3.0 编不过；6.4.0RC1 编译通过，但仍是 beta | 等 6.4.0 稳定版 |
| xdebug | 3.5.3 | 3.5.3 的 `config.m4` 拒绝 `>= 8.6`；3.6.0alpha1 编译通过，但只是预发布 | 等 3.6.0 稳定版 |
| swoole | 6.3.0 | 仍 `#error "require PHP version 8.5 or earlier"`；master README 也只写 8.2–8.5 | 不支持 8.6 |
| apcu | 5.1.28 | 编译失败：`XtOffsetOf`、`php_verror` 签名 | 等新版本 |
| msgpack | 3.0.1 | 编译失败：`zend_string` 比较不兼容 | 等新版本 |
| igbinary | 3.2.17RC1 | 编译失败：`zval_dtor` | 等新版本 |
| protobuf | 5.37.0RC3 | 编译失败：`zval_dtor` | 等新版本 |
| imagick | 3.8.1 | 源码仍用 `zval_dtor` | 不支持 |
| memcached | 3.4.0 | 源码仍用 `zval_dtor` | 不支持 |
| yaml | 2.3.0 | 源码仍用 `zval_dtor` | 不支持 |
| amqp | 2.2.0 | 源码仍用 `XtOffsetOf` / `INI_INT` | 不支持 |
| rdkafka | 6.0.5 / 7.0.0alpha1 | 源码仍用 `XtOffsetOf` / `zval_dtor` | 不支持 |

8.6 bundle 的顶层扩展默认线目前只有 `ssh2 = "1.6"`；其余扩展没有写入 `LINES`，显式选择会停在选线阶段，不会产出必失败的计划。

## 4. 本轮变更

- `crates/upstream/src/version.rs`：预发布后缀排在正式版之前，补丁字母保持不变；单测覆盖 `8.6.0RC3 < 8.6.0`。
- `recipes/catalog.toml`：`8.6` 线追加 `8.6.0`；`[ssh2]` 增加 `1.6` 线。
- `recipes/components/php/ssh2.star`：增加 `LINES["1.6"]`，产品范围 `8.6`。
- `recipes/bundles/php/8.6.toml`：增加 `ssh2 = "1.6"`。
- `recipes/products/php/8.6.star`：注释与核对依据改为 `php-8.6.0`。
- 文档同步：`docs/ADD-RECIPE.md`、`docs/CATALOG-UPDATE.md`、`docs/TOOLS.md`、`docs/plan/SWOOLE.md`。

## 5. 不在本轮范围

- redis 6.4.0 稳定版发布后再加 `6.4` 线；当前 6.4.0RC1 不写入 catalog。
- xdebug 3.6.0 稳定版发布后再加 `3.6` 线；当前 3.6.0alpha1 不写入 catalog。
- swoole 需要等上游在正式源码里解除 PHP 8.6 的版本上限。
- apcu、msgpack、igbinary、protobuf、imagick、memcached、yaml、amqp、rdkafka 需要等上游适配 PHP 8.6 的新版本。

## 6. 验收

本地已执行：

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

cargo run -- catalog update php --dry-run
cargo run -- plan php@8.6
cargo run -- install php@8.6
cargo run -- add php@8.6 ssh2
```

结果：

- `catalog update php --dry-run` 的 8.6 线显示「无更新 8.6.0」。
- `plan php@8.6` 的 tree 是 `.phprt/php-8.6.0`。
- `install php@8.6` 成功，`./myphp-8.6 -v` 输出 `PHP 8.6.0`。
- `add php@8.6 ssh2` 成功，`./myphp-8.6 -m` 包含 `ssh2`，`ssh2_connect()` 存在。
- 默认模块包含 `curl`、`mbstring`、`mysqli`、`openssl`、`pcntl`、`pdo_mysql`、`pdo_sqlite`、`shmop`、`sockets`、`sodium`、`sqlite3`、`sysvmsg`、`sysvsem`、`sysvshm`、`zip`。
