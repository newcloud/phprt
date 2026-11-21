# Swoole 构建开关

Swoole 有四条版本线（4.8 / 5.1 / 6.0 / 6.2），`config.m4` 的选项差别很大。本文是
`recipes/components/php/swoole.star` 的盘点记录和改造方案：哪些开关做成 feature、哪些固定在
`configure`、哪些进 `ignored`，默认值放在哪个 bundle。PHP 8.6 不提供 swoole。

只覆盖 Swoole 自己的编译开关。PHP 扩展、依赖库的开关处置见 [FEATURES.md](FEATURES.md)。

## 1. 盘点方法

`phprt audit php@<系列>` 读扩展源码里的 `config.m4`，用 `PHP_ARG_ENABLE` /
`PHP_ARG_WITH` / `AC_ARG_*` 得到上游选项，再按选项名（去掉 `enable-` / `with-` 前缀）
比对 recipe 的 `features`、`configure`、`ignored`。所以：

- 每个 PHP 系列只盘点它自己绑定的那条线：7.4 → 4.8，8.0 → 5.1，8.1 ~ 8.3 → 6.0，
  8.4 / 8.5 → 6.2；8.6 不提供 swoole。
- 同一个 recipe 要同时满足四条线的选项清单，`ignored` 必须按线声明；否则 audit
  会把「这条线没有的选项」报成 stale。
- 上游某个开关在新线里删掉后，`ignored` 里的旧条目要从这条线上撤掉。

## 2. 选项矩阵

`●` 表示该线的 `config.m4` 有这个选项。

| 上游选项 | 4.8 | 5.1 | 6.0 | 6.2 | 作用 |
| --- | --- | --- | --- | --- | --- |
| `--enable-swoole` | ● | ● | ● | ● | 扩展总开关（默认开） |
| `--enable-openssl` | ● | ● | ● | | 启用 SSL |
| `--with-openssl-dir` | ● | ● | ● | ● | 指定 OpenSSL 路径 |
| `--enable-swoole-curl` | ● | ● | ● | ● | hook PHP curl |
| `--enable-cares` | ● | ● | ● | ● | 异步 DNS（c-ares） |
| `--enable-sockets` | ● | ● | ● | ● | hook PHP sockets，开 `Swoole\Client::getSocket()` |
| `--enable-swoole-sqlite` | | ● | ● | ● | 协程 PDO SQLite |
| `--enable-http2` | ● | | | | 编译 HTTP/2 支持（nghttp2 随包） |
| `--enable-mysqlnd` | ● | ● | ● | ● | 声明 mysqlnd 依赖 |
| `--enable-swoole-pgsql` | | ● | ● | ● | 协程 PDO PgSQL |
| `--with-swoole-odbc` | | ● | ● | ● | 协程 PDO ODBC |
| `--with-swoole-oracle` | | ● | ● | ● | 协程 PDO Oracle |
| `--with-swoole-firebird` | | | | ● | 协程 PDO Firebird |
| `--with-swoole-ssh2` | | | | ● | 协程 SSH2（捆绑 libssh2） |
| `--enable-swoole-ftp` | | | | ● | 协程 FTP |
| `--enable-brotli` | ● | ● | ● | ● | HTTP 压缩（探测系统库 / pkg-config） |
| `--with-brotli-dir` | ● | ● | ● | ● | 指定 Brotli 路径 |
| `--enable-zstd` | | | ● | ● | HTTP 压缩（pkg-config libzstd） |
| `--with-nghttp2-dir` | ● | ● | ● | ● | 指定 nghttp2 路径 |
| `--enable-iouring` | | | ● | ● | io_uring 事件循环 |
| `--with-liburing-dir` | | | | ● | 指定 liburing 路径 |
| `--enable-uring-socket` | | | | ● | io_uring socket |
| `--enable-swoole-thread` | | | ● | ● | 线程模式 |
| `--enable-thread-context` | ● | ● | ● | ● | 线程上下文 |
| `--enable-swoole-coro-time` | | ● | ● | ● | 统计协程执行耗时 |
| `--enable-swoole-stdext` | | | | ● | swoole-cli 专用（experimental） |
| `--enable-swoole-json` | ● | | | | 只给 `swoole_substr_json_decode()` |
| `--with-jemalloc-dir` | ● | ● | | | 换 jemalloc 分配器 |
| `--enable-asan` / `--enable-debug` / `--enable-debug-log` / `--enable-trace-log` / `--enable-swoole-coverage` / `--enable-swoole-dev` | ● | ● | ● | ● | 调试、覆盖率、开发者模式 |

## 3. 处置

| 选项 | 处置 | 落地 |
| --- | --- | --- |
| `--enable-swoole` | 固定 | `configure`；4.8 / 5.1 补上，6.0 / 6.2 已有 |
| `--enable-openssl` | 固定 | 4.8 / 5.1 的 `configure`；路径由 phprt 的 `CPPFLAGS` / `LDFLAGS` 提供 |
| `--enable-openssl`（6.0） | ignored | 6.0 用 `--with-openssl-dir` 就够，6.2 已删除 |
| `--with-openssl-dir` | 固定 / ignored | 6.0 / 6.2 的 `configure`；4.8 / 5.1 不用它，路径走环境变量 |
| `--enable-swoole-curl` | 固定 | `dep_configure = {"libcurl": [...]}` |
| `--enable-cares` | feature `cares` | 已有；`needs=["c-ares"]`，默认关 |
| `--enable-sockets` | feature `sockets` | 默认开（`[component_presets].swoole`） |
| `--enable-swoole-sqlite` | feature `sqlite` | `needs=["sqlite"]`，默认开；4.8 没有这个开关，不定义该 feature |
| `--enable-http2`（4.8） | 固定 | 4.8 的 `configure` |
| `--enable-brotli` | feature `brotli` | `needs=["brotli"]`，默认关；未选中时基础 `configure` 的 `--disable-brotli` 关掉自动探测 |
| `--with-brotli-dir` | ignored | `--enable-brotli` 走 pkg-config 就够，不需要单独指定目录 |
| `--enable-zstd` | feature `zstd` | `needs=["zstd"]`，默认关；只 6.0 / 6.2 定义 |
| `--with-nghttp2-dir` | ignored | nghttp2 源码随包编，不需要外部路径 |
| `--enable-mysqlnd` | ignored | 6.x 只加 `ZEND_MOD_REQUIRED("mysqlnd")`；`Coroutine\MySQL` 已移除 |
| `--enable-swoole-pgsql` | ignored | 少用：`Swoole\Coroutine\PostgreSQL` 客户端 6.0 起已移除，官方建议改用协程化的 PDO；PG 走 PHP 自己的 `pdo_pgsql`（见 §4.5） |
| `--with-swoole-odbc` | ignored | `缺依赖（未排期）：unixODBC` |
| `--with-swoole-oracle` | ignored | 少用：需要 Oracle 客户端 |
| `--with-swoole-firebird` | ignored | 少用：需要 Firebird 客户端 |
| `--with-swoole-ssh2` | ignored | 少用：与 PHP ssh2 扩展冲突（`ZEND_MOD_CONFLICTS("ssh2")`） |
| `--enable-swoole-ftp` | ignored | 少用：与 PHP ftp 扩展冲突（`ZEND_MOD_CONFLICTS("ftp")`） |
| `--enable-iouring` / `--with-liburing-dir` / `--enable-uring-socket` | ignored | 少用：上游标注实验性，要 liburing 和较新内核 |
| `--enable-swoole-thread` / `--enable-thread-context` | ignored | 少用：只在 ZTS 构建有意义，phprt 用 NTS |
| `--enable-swoole-coro-time` | ignored | 少用：统计协程耗时，有额外开销 |
| `--enable-swoole-stdext` | ignored | 调试用途：experimental，只给 swoole-cli |
| `--enable-swoole-json` | ignored | 少用：只影响 `swoole_substr_json_decode()`，4.8 之后已删 |
| `--with-jemalloc-dir` | ignored | 少用：换分配器要另编 jemalloc |
| `--enable-asan` / `--enable-debug` / `--enable-debug-log` / `--enable-trace-log` / `--enable-swoole-coverage` / `--enable-swoole-dev` | ignored | 调试用途 |

## 4. 改动

### 4.1 `recipes/components/php/swoole.star`

```python
LINES = {
    "4.8": {"php": ["7.4", "8.0", "8.1", "8.2"], "openssl": ["1.1.1"]},
    "5.1": {"php": ["8.0", "8.1", "8.2", "8.3"], "openssl": ["1.1.1", "3.0", "3.5"]},
    "6.0": {"php": ["8.1", "8.2", "8.3", "8.4"], "openssl": ["1.1.1", "3.0", "3.5"]},
    "6.2": {"php": ["8.4", "8.5"], "openssl": ["1.1.1", "3.0", "3.5", "4.0"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]
    line = self["line"]

    needs = ["libcurl"]
    configure = []
    if "openssl" in LINES[line]:
        needs = ["libcurl", "openssl"]
        if line in ("6.0", "6.2"):
            # 6.0 起用 --with-openssl-dir 启用 SSL；6.2 删掉了旧的 --enable-openssl。
            configure = [
                "--enable-swoole",
                "--with-openssl-dir={dep:openssl}",
                "--disable-brotli",
            ]
        else:
            # 4.8 / 5.1 仍用 --enable-openssl，路径由 CPPFLAGS / LDFLAGS 提供。
            # --disable-brotli：上游默认 yes 且探测宿主库，不关掉构建不可复现。
            configure = ["--enable-swoole", "--enable-openssl", "--disable-brotli"]

    # 4.8 的 HTTP/2 是可选编译开关；nghttp2 源码随包，不需要新依赖。
    if line == "4.8":
        configure.append("--enable-http2")

    features = {
        "cares": feature(
            needs = ["c-ares"],
            configure = ["--enable-cares"],
        ),
        # socket 相关 API 和 PHP sockets 的协程 hook；PHP sockets 扩展在默认集里。
        "sockets": feature(
            configure = ["--enable-sockets"],
        ),
        # 基础 configure 里始终有 --disable-brotli；选中时追加 --enable-brotli，
        # autoconf 对同一选项以最后一次为准。
        "brotli": feature(
            needs = ["brotli"],
            configure = ["--enable-brotli"],
        ),
    }
    # 下面这个开关不是四条线都有；没有该开关的线上不定义 feature。
    if line != "4.8":
        features["sqlite"] = feature(
            needs = ["sqlite"],
            configure = ["--enable-swoole-sqlite"],
        )
    if line in ("6.0", "6.2"):
        features["zstd"] = feature(
            needs = ["zstd"],
            configure = ["--enable-zstd"],
        )

    ignored = {
        "--enable-asan": "调试用途：只用于测试",
        "--enable-debug": "调试用途：不用于发布构建",
        "--enable-debug-log": "调试用途：日志开销大",
        "--enable-thread-context": "少用：只在 ZTS 构建有意义，phprt 用 NTS",
        "--enable-trace-log": "调试用途：日志开销大",
        "--enable-swoole-coverage": "调试用途：代码覆盖率",
        "--enable-swoole-dev": "调试用途：开发者模式，不用于发布",
        "--enable-mysqlnd": "少用：6.x 只加模块依赖声明，Coroutine\\MySQL 已移除",
        "--with-nghttp2-dir": "少用：nghttp2 源码随包编，不需要外部路径",
        "--with-brotli-dir": "少用：--enable-brotli 走 pkg-config 就够",
    }
    if line in ("4.8", "5.1"):
        ignored["--with-jemalloc-dir"] = "少用：换分配器要另编 jemalloc"
        ignored["--with-openssl-dir"] = "路径由 phprt 的 CPPFLAGS / LDFLAGS 提供"
    if line == "5.1":
        ignored["--enable-swoole-coro-time"] = "少用：统计协程耗时，有额外开销"
        ignored["--enable-swoole-pgsql"] = "少用：Swoole 的 PostgreSQL 客户端已移除，PG 走 PHP 的 pdo_pgsql"
        ignored["--with-swoole-odbc"] = "缺依赖（未排期）：unixODBC"
        ignored["--with-swoole-oracle"] = "少用：需要 Oracle 客户端"
    if line == "4.8":
        ignored["--enable-swoole-json"] = "少用：只影响 swoole_substr_json_decode()，后续线已删"
    if line == "6.0":
        ignored["--enable-openssl"] = "旧开关：6.0 用 --with-openssl-dir 就够了，6.2 已删除"
    if line in ("6.0", "6.2"):
        ignored["--enable-iouring"] = "少用：上游标注实验性，要 liburing 和较新内核"
        ignored["--enable-swoole-thread"] = "少用：只在 ZTS 构建有意义，phprt 用 NTS"
        ignored["--enable-swoole-coro-time"] = "少用：统计协程耗时，有额外开销"
        ignored["--enable-swoole-pgsql"] = "少用：Swoole 的 PostgreSQL 客户端已移除，PG 走 PHP 的 pdo_pgsql"
        ignored["--with-swoole-odbc"] = "缺依赖（未排期）：unixODBC"
        ignored["--with-swoole-oracle"] = "少用：需要 Oracle 客户端"
    if line == "6.2":
        ignored["--with-liburing-dir"] = "少用：随 iouring 一起不暴露"
        ignored["--enable-uring-socket"] = "少用：随 iouring 一起不暴露"
        ignored["--enable-swoole-ftp"] = "少用：与 PHP ftp 扩展冲突"
        ignored["--enable-swoole-stdext"] = "调试用途：experimental，只给 swoole-cli"
        ignored["--with-swoole-firebird"] = "少用：需要 Firebird 客户端"
        ignored["--with-swoole-ssh2"] = "少用：与 PHP ssh2 扩展冲突"

    return component(
        name = "swoole",
        modules = ["swoole"],
        link = ["shared"],
        needs = needs,
        tools = ["autoconf"],
        dep_configure = {"libcurl": ["--enable-swoole-curl"]},
        features = features,
        ignored = ignored,
        ...
    )
```

要点：

- `features` 先写成公共部分，再按线补 `sqlite` / `zstd`。这两处是本文里仅有的
  按线增删 feature 的地方：4.8 没有 `--enable-swoole-sqlite`，`zstd` 只有 6.0 / 6.2 有。
  `requires` 只能按 PHP 系列过滤，而 4.8 / 5.1 / 6.0 的 PHP 范围互相重叠，表达不了。
  `audit` 会按线检查，没有该开关的线上不会漏账。
  没定义 feature 的线上，CLI 会直接报「没有 feature <名字>」；用 `version:` 把 8.x 的
  preset 套到不支持它的线上时，preset 会丢弃这个 feature 并打印「注意」。
  如果以后要按线过滤 feature，应该给 `requires` 加 `lines`，不要继续加 if；那是契约
  改动，本次不做。
- `ignored` 按线分组同理。`audit` 对「ignored 但上游没有这个选项」报 stale 并失败，
  所以 6.x 才有的开关不能写在 4.8 / 5.1 的清单里。

### 4.2 bundle 默认值

`recipes/bundles/php/8.0.toml` ~ `8.5.toml` 的 `[component_presets]` 各加一行：

```toml
swoole = { features = ["sockets", "sqlite"] }
```

`recipes/bundles/php/7.4.toml` 现在没有 `[component_presets]`，新增这个表（4.8 只有 `sockets`）：

```toml
[component_presets]
swoole = { features = ["sockets"] }
```

`recipes/components/php/swoole.star` 的 `LINES["6.2"].product` 改成 `["8.4", "8.5"]`：PHP 8.6 不提供
swoole（原因见 §4.4）。`bundles/php/8.6.toml` 里的 `swoole = "6.2"` 也要一起删掉：bundle 的默认线
会参与选线校验，留着会让 `list php@8.6` 直接报「swoole 6.2 不支持产品 8.6」。删掉后
`add php@8.6 swoole` 会停在选线阶段：不写版本线会要求指定，指定 6.2 会报
「swoole 6.2 不支持产品 8.6」，不会产出必失败的计划。swoole 本来就不在
8.6 的默认组件里。

- 只影响 `--component swoole` / `add php@8.5 swoole`，不会让 swoole 变成默认安装的扩展。
- 8.0 走 5.1 线，8.1 ~ 8.3 走 6.0 线，8.4 / 8.5 走 6.2 线，三条线都有 `sockets` / `sqlite`。
- `brotli` / `zstd` 不写进 preset：它们要新增依赖库，默认关。
- 显式写 `feature:` 会覆盖 preset；`feature:[]` 清空。
- 用户用 `version:` 换线时，preset 里该线没有的 feature 会被丢弃并打印「注意」，
  不会中断。

### 4.3 新增依赖 recipe

三个可选 feature 各要一个新 dep。版本线、补丁号、下载地址只写 `recipes/catalog.toml`；
recipe 只写构建方式。

| 依赖 | 构建系统 | 关键参数 | 产物 | 用于 |
| --- | --- | --- | --- | --- |
| `brotli` | CMake | `-DCMAKE_INSTALL_PREFIX={prefix}`、`-DCMAKE_INSTALL_LIBDIR=lib`、`-DBUILD_SHARED_LIBS=ON` | `lib/libbrotlienc.so` | `--enable-brotli` |
| `zstd` | 自带 Makefile | `make -C lib`、`make -C lib install PREFIX={prefix}` | `lib/libzstd.so` | `--enable-zstd` |
| `libpq` | autoconf | `./configure --prefix={prefix}`，关掉 readline / zlib / ICU，只编客户端库 | `lib/libpq.so`、`bin/pg_config` | PHP `pdo_pgsql` 的 `--with-pdo-pgsql` |

- 三个库都安装 pkg-config 文件；phprt 已经把依赖前缀下的 `lib/pkgconfig` 加进
  `PKG_CONFIG_PATH`，swoole 的 `--enable-brotli` / `--enable-zstd` 都靠它找库。
- `libpq` 只编客户端：`configure` 之后 `make -C src/interfaces/libpq` +
  `make -C src/interfaces/libpq install`，不编服务端；还要
  `make -C src/bin/pg_config install` 和 `make -C src/include install`，
  因为 PHP 的 `pdo_pgsql` 靠 `bin/pg_config` 定位 libpq。PostgreSQL 各版本的
  `--without-*` 开关名不完全一样，落地时按 catalog 选定的版本核对。
- 三个 recipe 都不写 PHP 关系；能不能用由消费扩展 feature 所在的线决定。

### 4.4 8.6 不提供 swoole

6.2.5 的 `ext-src/php_swoole_private.h` 在 `PHP_VERSION_ID >= 80600` 直接
`#error "require PHP version 8.5 or earlier"`。做法是把 8.6 从 `LINES["6.2"]` 撤掉，
并删掉 `bundles/php/8.6.toml` 里的 `swoole = "6.2"`；否则 bundle 的默认线会在
`list php@8.6` 这类操作里被校验并直接报错。删掉后显式选 swoole 会停在选线阶段
（不写版本线会要求指定，指定 6.2 会报不支持产品 8.6），而不是产出一个必失败的计划。
等 6.3 有版本线时再把 8.6 加回。

### 4.5 pdo_pgsql（PG 的方向）

Swoole 的 PostgreSQL 协程客户端上游已移除，swoole 不再暴露 pgsql feature；PG 的
支持改为 PHP 自己的 `pdo_pgsql` 内置扩展，照 `pdo_sqlite` 的写法：

```python
PRODUCT_DEP_LINES = {
    series: {"libpq": ["18"]}
    for series in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        name = "pdo_pgsql",
        modules = ["pdo_pgsql"],
        modes = ["static"],
        needs = ["libpq"],
        dep_configure = {"libpq": []},
        configure = ["--with-pdo-pgsql={dep:libpq}"],
        ...
    )
```

- `pdo` 是 PHP 内置，不需要额外声明；`pdo_pgsql` 不进 `[default_components]`，
  用 `--component pdo_pgsql` 显式选择。
- 各系列 `products/php/<系列>.star` 的 `BUNDLED` 已有 `pdo_pgsql`，
  `BUNDLED_DISABLE` 不用加（PHP 默认不编它）。
- `pdo_pgsql` 是静态内置扩展；swoole 只 hook PDO 层，不提供 PG 的协程客户端。

## 5. 依赖与运行期

### 5.1 `sockets`

- 编译期要 PHP 的 `ext/sockets/php_sockets.h`；phprt 装好的 PHP 在
  `include/php/ext/sockets/` 里有这个头（`phpincludedir` 在 include 路径里）。
- 运行期 `swoole.so` 声明 `ZEND_MOD_REQUIRED("sockets")`，PHP 的 sockets 扩展
  必须加载，否则 swoole 直接加载失败。
- 8.0 ~ 8.5 的 `[default_components]` 都有 `sockets = {}`。如果用户显式
  `--disable-default-component sockets` 又启用 swoole，上游 configure 会报
  `cannot find php_sockets.h`，这是预期报错；契约目前没有「feature 依赖 PHP 扩展」
  字段，本次不新增。

### 5.2 `sqlite`

- `needs=["sqlite"]` 引入依赖库 `sqlite`（catalog 只有 3.54 一条线，自动绑定，
  不用写进 bundle `[deps]`）。
- 编译期用 `PKG_CHECK_MODULES([SQLITE], [sqlite3 >= 3.7.7])`，`phprt` 会把依赖前缀
  加进 `PKG_CONFIG_PATH`。
- 只要 PDO 本体（PHP 一直内置，`pdo` 扩展默认编入）。**不需要** PHP 的 `pdo_sqlite`
  扩展：5.1 / 6.0 / 6.2 都自带一份 `pdo_sqlite` 驱动源码（`thirdparty/php*/pdo_sqlite`
  或 `thirdparty/pdo_sqlite`），注册成 `sqlite` 驱动并走协程。
- 可选：`sqlite3_column_table_name` 这类元数据 API 由依赖库的 `column_metadata`
  feature 提供。8.0 ~ 8.5 的 `sqlite3` / `pdo_sqlite` preset 已经打开它，同一次安装
  共用同一份 `sqlite` 库；用户只装 swoole 不装 sqlite3 扩展时，库仍能编过，只是
  `HAVE_SW_SQLITE3_COLUMN_TABLE_NAME` 不定义。

### 5.3 brotli 的探测与关闭

4.8 / 5.1 的 `--enable-brotli` 默认 `yes`，用 `AC_CHECK_LIB` 直接探测宿主机的
`brotlienc` / `brotlidec`。4.8.13 的构建产物里 `ldd swoole.so` 会带出
`/lib64/libbrotli*.so`，说明产物随宿主机变化。6.0 / 6.2 默认 `auto` + pkg-config，
同样会探测。

所以基础 `configure` 始终带 `--disable-brotli`；`brotli` feature 选中时追加
`--enable-brotli`，autoconf 对同一个选项以最后一次出现为准。没有用
`--with-brotli-dir`：4.8 / 5.1 的 dir 分支只加 include/lib，不走 `--enable-brotli`
的探测逻辑，压缩支持的开关可能对不上。

### 5.4 zstd / libpq

- `--enable-zstd` 默认 `no`，打开后用 `PKG_CHECK_MODULES([ZSTD], [libzstd >= 1.4.0])`；
  依赖 `zstd` 通过 `make -C lib install` 安装 `libzstd.pc`。
- `libpq` 的消费者是 PHP 的 `pdo_pgsql`（§4.5），不是 swoole。libpq 构建会连带跑
  PG 的 perl 脚本生成 catalog 头，需要 `FindBin`、`File::Compare`、`Getopt::Long`
  等核心模块；宿主 perl 不全时按 tools 的 `perl` 预检提示装完整的 perl 包
  （见 `docs/BUILD.md` §7）。

## 6. 默认值判据

- `sockets`：不增加依赖（PHP sockets 是默认扩展），只是多出 hook 和
  `getSocket()`，命中「不增加新依赖、只是多一个能力 → 默认开」。
- `sqlite`：依赖库 `sqlite` 已经在默认集（`sqlite3` / `pdo_sqlite` 都要编它），
  命中「依赖已经在默认集 → 默认开，并和依赖一起写」。
- `cares`：引入新的 `c-ares` 依赖，默认关。
- `brotli` / `zstd`：都要新增依赖库，默认关。
- 两个默认值只写在 `[component_presets]`，不写进 recipe。

## 7. 验收

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets

# 每个系列都要 0；swoole 源码要先解压（装过一次）
phprt audit php@7.4
phprt audit php@8.0
phprt audit php@8.5

# 预设：两个开关都在，且 dep:sqlite 进构建顺序
phprt --component swoole plan

# 显式清空：两个开关都不出现
phprt --component 'swoole=feature:[]' plan

# 可选 feature：两个依赖都进构建顺序
phprt --component 'swoole=feature:[sockets,sqlite,brotli,zstd]' plan

# pdo_pgsql：libpq 进构建顺序，PHP configure 带 --with-pdo-pgsql
phprt --component pdo_pgsql plan

# 4.8 线：http2 在 configure 里，没有 sqlite / zstd
phprt plan php@7.4 --component 'swoole=feature:[brotli]'

# 5.1 线：没有 zstd
phprt plan php@8.0 --component swoole
phprt plan php@8.0 --component 'swoole=feature:[brotli]'

# 8.6 不再提供 swoole：必须失败
phprt audit php@8.6 --component swoole plan
```

计划输出里应能看到 `--enable-sockets`、`--enable-swoole-sqlite`（8.0 及以上）、
`--enable-http2`（7.4）、`dep:sqlite`；选中可选 feature 时还要有 `--enable-brotli`、
`--enable-zstd`（6.0 / 6.2），以及 `dep:brotli`、`dep:zstd`。`pdo_pgsql` 方案里
出现 `dep:libpq` 和 `--with-pdo-pgsql=<libpq 前缀>`。真构建至少覆盖两条线：

```bash
phprt install 8.5 --component swoole
phprt install 8.0 --component swoole     # 5.1 线

php --ri swoole | grep -E 'sockets|coroutine_sqlite'
php -r 'var_dump(method_exists(Swoole\Client::class, "getSocket"));'
php -r 'Swoole\Coroutine\run(function () {
    $pdo = new PDO("sqlite::memory:");
    $pdo->exec("create table t (id integer primary key, v text)");
    $pdo->exec("insert into t (v) values (\"a\")");
    var_dump($pdo->query("select v from t")->fetchColumn());
});'
```

已有安装根不会因为 bundle preset 变化自动重编扩展；验证默认值要用干净根，
或显式写 `--component 'swoole=feature:[sockets,sqlite]'`。

## 8. 未决

1. **`sockets` 依赖 PHP 扩展**：`requires` 只支持 PHP 系列、依赖线、宿主机命令，
   表达不了「要 PHP sockets 扩展」。当前靠默认集保证；如果以后要严格校验，再考虑
   加契约字段（本次不做）。
2. **brotli 的参数顺序**：基础 `--disable-brotli` + 选中时 `--enable-brotli`，
   依赖 autoconf「同一选项以最后一次为准」。要让 feature 未选中时自动加关闭参数，
   应该给 ext 的 feature 开放 `disable`（现在只允许 dep 用），那是契约改动。
3. **libpq 的安装面**：`pdo_pgsql` 靠 `bin/pg_config` 定位 libpq，所以 libpq recipe
   要装 `src/bin/pg_config` 和 `src/include` 的头；PG 18 不再生成 `libpq.pc`，
   不要再指望 pkg-config。

