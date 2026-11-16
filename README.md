# phprtv3

phprt v3 的原型：**Starlark 写 recipe，Rust 做版本解析、求解、下载和构建**。
模型是三层：产品（PHP、nginx）、组件（PHP 扩展、nginx 模块）、依赖库（OpenSSL、PCRE2）。

`plan` 算出依赖闭包、拓扑顺序和每个节点的参数。`install` / `update` 默认把方案里的
源码包下载到 `.phprt/downloads/`，按构建顺序解压到 `build/<名字>/<补丁>/`，编译安装到
`.phprt/<产品>-<版本>/`，最后写状态。PHP 还会生成项目入口 `./myphp-<系列>`。
`--downloads` 只下载源码，不解压、不编译、不写状态。

产品自己的行为写在 `recipes/products/<产品>/_profile.star`：需要哪些宿主机命令和最低
版本、安装后写哪些文件、入口用什么模板。Rust 只执行这些声明，不按产品写分支。规则见
[docs/PRODUCT.md](docs/PRODUCT.md)。

构建环境、日志、跳过规则和运行时库整理见 [docs/BUILD.md](docs/BUILD.md)。

版本和默认策略分成三层：

```text
bundle       php/8.5：swoole 默认线 6.2，[deps] 里 openssl 库 3.5；默认启用常用内置扩展
catalog      6.2 → 6.2.5 + 下载地址；3.5 → 3.5.9 + 下载地址
recipe       6.2 支持产品系列 8.4/8.5/8.6，需要 libcurl/openssl，怎么编
```

补丁更新只改 `recipes/catalog.toml`，bundle 和 recipe 不会过期。

设计细节见 [docs/DESIGN.md](docs/DESIGN.md)。

## 快速开始

先构建一次二进制，之后直接用 `phprt`；下面的例子写 `cargo run --`，两者等价：

```bash
cargo build --release
./target/release/phprt install php@8.5
```

全局参数：`--recipes <目录>`（默认 `recipes`）、`--root <目录>`（默认 `.`，状态写在
`<root>/.phprt` 下）。所有命令都写 `产品@系列`；一条命令只处理一个产品。

```bash
cargo run -- plan php@8.5                  # 只看方案：依赖、顺序、参数
cargo run -- install php@8.5               # PHP 8.5.12 + 默认内置扩展
./myphp-8.5 -v                             # 用项目自己的 PHP

cargo run -- install php@8.5 --downloads   # 只下载源码包
cargo run -- update php@8.5                # 在 8.5 线上重新解析补丁
cargo run -- audit php@8.5                 # 对照上游选项清单
cargo run -- list                          # 已安装的产品
cargo run -- add php@8.5 swoole            # 加入方案（待安装）
cargo run -- add php@8.5 xdebug
cargo run -- install php@8.5               # 装方案里的组件
cargo run -- remove php@8.5 xdebug
cargo run -- list php@8.5

cargo run -- install nginx@1.29            # nginx 1.29.8 + PCRE2 + OpenSSL
.phprt/nginx-1.29.8/sbin/nginx -V
cargo run -- add nginx@1.29 lua-nginx-module
cargo run -- install nginx@1.29            # 动态模块 modules/ngx_http_lua_module.so
```

`add` 只把组件写进方案，要再跑一次 `install` 才会编译；`remove` 立刻删掉该组件的 ini。

只装 PHP 本体：

```bash
cargo run -- install php@8.5 \
  --disable-default-component openssl \
  --disable-default-component curl \
  --disable-default-component libxml
```

gd 只在 PHP 8.0 ~ 8.5 下加载，且不在默认组件集里；选中 gd 时使用
`recipes/bundles/php/8.5.toml` 的 `[component_presets].gd`，自动带 JPEG/WebP/FreeType。
PNG 由 libpng 固定提供，`avif` 默认关闭。`dep:jpeg[simd]` 需要宿主机有 `nasm`。

## 构建工具

编译前会检查宿主机命令，清单来自 recipe，不写死在代码里：

| 来源 | 内容 |
| --- | --- |
| 产品 profile 的 `tools` | 这个产品总是需要的命令，可带最低版本 |
| dep / 组件的 `host_tools` | 只有本次构建包含它时才检查，如 `cmake`、`nasm` |

当前产品至少需要：

| 产品 | 命令 |
| --- | --- |
| php | `gcc` ≥ 9、`g++` ≥ 9、`make` ≥ 4.0、`pkg-config` ≥ 0.29、`perl` ≥ 5.20、`m4` ≥ 1.4、`patchelf` ≥ 0.13、`ar`、`ranlib`、`sh` |
| nginx | `gcc` ≥ 8、`make` ≥ 4.0、`perl` ≥ 5.20、`patchelf` ≥ 0.13、`ar`、`ranlib`、`sh` |

版本不够或命令不存在都会在下载源码之前失败：

```text
error: 需要 gcc >= 9，当前 1.0.0
error: 缺少构建工具: patchelf
```

同一个命令出现多次时取最严的一条（有最低版本的优先，都有时留更高的），所以
`host_tools` 里的 `"nasm"` 不会抹掉别处声明的 `tool("nasm", min = "2.13")`。
`--downloads` 不做工具检查。探测和比较规则见 [docs/PRODUCT.md](docs/PRODUCT.md)。

## 项目入口

PHP 安装完会在项目根写 `./myphp-<系列>`；项目里只有一个 PHP 系列时再写一份 `./myphp`。
内容来自 `recipes/products/php/entry.sh`，安装时替换模板里的 `@@TREE@@` / `@@TARGET@@`。

```bash
./myphp-8.5 -v
./myphp-8.5 -m                      # 已加载的扩展
./myphp-8.5 -i | grep 'Scan this dir'   # ini 扫描目录
```

nginx 不生成入口，直接跑安装树里的二进制：

```bash
.phprt/nginx-1.29.8/sbin/nginx -V
```

## 目标

```bash
phprt install php@8.5       # 一个产品
phprt install nginx@1.29
phprt list                  # 再看装了哪些
```

不支持一次装多个产品：

```text
error: install 只接受一个产品，例如 phprt install php@8.5；多个产品请分多条命令
```

也没有默认版本或当前上下文；`phprt install 8.5`、`phprt add swoole` 都直接报错。

## 版本线

```bash
# 不写 version：用 php/8.5 bundle 的默认线（swoole 6.2）
cargo run -- add php@8.5 swoole

# 显式选线：6.0.x 的最新补丁从 catalog 取
cargo run -- install php@8.2 --component swoole=version:6.0

# 只改依赖库的线，不额外安装组件
cargo run -- install php@8.5 --component swoole --dep openssl=4.0

# 不支持当前产品的线直接失败
cargo run -- install php@8.5 --component swoole=version:4.8
# error: swoole 4.8 不支持产品 8.5（可用：6.2）
```

`version:` 只接受线（`6.2`），不接受补丁（`6.2.5`）；补丁由 catalog 决定。

多个组件约束同一个库时取交集。bundle 默认线冲突且只有一个候选时会自动切换并打印
提示；显式 `--dep` 冲突则直接报错。

## recipe

recipe 只声明版本关系、依赖和构建参数；版本号、下载地址从 ctx 注入：

```python
# recipes/components/php/swoole.star
LINES = {
    "6.2": {
        "product": ["8.4", "8.5", "8.6"],
        "openssl": ["1.1.1", "3.0", "3.5", "4.0"],
    },
}


def target(ctx):
    self = ctx["self"]
    line = self["line"]
    needs = ["libcurl"]
    configure = []
    if "openssl" in LINES[line]:
        needs = ["libcurl", "openssl"]
        configure = ["--enable-swoole", "--with-openssl-dir={dep:openssl}", "--disable-brotli"]
    return component(
        name="swoole",
        product="php",
        source="external",
        modes=["shared"],
        needs=needs,
        tools=["autoconf"],
        dep_configure={"libcurl": ["--enable-swoole-curl"]},
        configure=configure,
        ini="27-swoole.ini",
    )

TARGET = target
```

- `LINES`：组件自己的线支持哪些产品系列、允许哪些依赖线。
- `PRODUCT_LINES`：bundled 组件的产品系列 → 允许的同名依赖库线（如 component:openssl）。
- `PRODUCT_DEP_LINES`：bundled 组件的产品系列 → 不同名依赖线（如 gd 的 libpng/jpeg）。
- dep recipe 不声明产品关系；库版本由 bundle/CLI/消费组件决定。
- `target(ctx)`：只负责构建参数，不再判断“能不能配”。
- `ctx["is_bundled"]`：由产品 recipe 的 `bundled` 推导，不在组件 recipe 里写 origin。
- `TARGET` 也可以是普通 dict；产品 recipe 必须是 dict。
- 挂载方式由产品 profile 定义：PHP `static` / `shared`，nginx `builtin` / `dynamic`。

产品 recipe 只写版本事实，共用行为放在同目录的 `_profile.star`：

```python
# recipes/products/php/_profile.star —— 所有 PHP 系列共用
PROFILE = product_profile(
    tools = [tool("gcc", min = "9"), tool("make", min = "4.0"), "ar"],
    modes = {"static": mode(strategy = "fold"), ...},
    post_install = [
        ensure_dir("etc/conf.d"),
        write_file("etc/php.ini", PHP_INI, if_missing = True),
        ensure_dir("etc/ssl"),
        copy_ca_bundle("etc/ssl/cert.pem", [...]),
        write_component_ini("etc/conf.d", register = "php-ini"),
    ],
    entry = "myphp-{series}",
    entry_template = "entry.sh",
    env = {"CC": "gcc", "CXX": "g++"},
)

# recipes/products/php/8.5.star
TARGET = product(name = "php", series = "8.5", ..., profile = PROFILE)
```

系列要改工具要求或编译参数时用 `extend_profile`：

```python
# recipes/products/php/8.0.star
profile = extend_profile(
    PROFILE,
    env = {"CFLAGS": "-O2 -std=gnu17 -Wno-error=incompatible-pointer-types"},
)
```

`tools` 按名字合并（后面的替换前面的），`env` 按 key 合并，其他字段传了才替换。
`_profile.star` 以 `_` 开头，不会被当成系列 recipe。完整字段见
[docs/CONTRACT.md](docs/CONTRACT.md) 和 [docs/PRODUCT.md](docs/PRODUCT.md)。

### 依赖集成与 feature

`dep:` 打开组件对某个依赖的集成，方括号里的 feature 转给该依赖：

```bash
cargo run -- install php@8.2 --component swoole=version:6.0,dep:libcurl[ssl,nghttp2]
```

- `dep:libcurl` 给 swoole 加 recipe 里 `dep_configure` 声明的参数（`--enable-swoole-curl`）。
- `[ssl,nghttp2]` 转给 libcurl，具体编译参数由 libcurl 的 `features` 决定。
- 不写 `dep:libcurl` → 开关和转发都不发生。

`feature:` 是组件自己的开关，和依赖无关：

```bash
cargo run -- install php@8.5 --component 'openssl=feature:[argon2]'
cargo run -- install php@8.5 --component 'openssl=feature:[]'   # 关掉默认开的 feature
```

- 必须带方括号；多个 feature 用逗号分隔。
- 有条件的能力在 recipe 里用 `requires` 声明；产品系列 / 依赖线不满足时选择直接报错。

### 占位符

| 占位符 | 含义 |
| --- | --- |
| `{tree}` / `{product}` | `.phprt/<产品>-<产品版本>` |
| `{deps}` | `{tree}/deps` |
| `{build}` | 当前节点的解压目录 |
| `{prefix}` | 当前节点的安装前缀 |
| `{dep:名字}` | 指定依赖的安装前缀，如 `{dep:openssl}` |
| `{source:名字}` | 对应 dep / component 的解压源码根 |

## 当前版本表

产品：PHP 7.4 / 8.0 / 8.1 / 8.2 / 8.3 / 8.4 / 8.5 / 8.6；nginx 1.29。

| 组件 | 线 | 默认使用 |
| --- | --- | --- |
| openssl | 1.1.1 / 3.0 / 3.5 / 4.0 | 8.5 用 3.5；4.0 允许但不默认 |
| pcre2 | 10.49 | 10.49（nginx） |
| luajit | 2.1 | OpenResty LuaJIT2 v2.1-20261107（lua-nginx-module） |
| lua-resty-core | 0.1 | 0.1.32（lua-nginx-module 运行期必需） |
| lua-resty-lrucache | 0.15 | 0.15（lua-resty-core 运行期必需） |
| lua-nginx-module | 0.10 | 0.10.29（nginx 动态模块） |
| swoole | 4.8 / 5.1 / 6.0 / 6.2 | 7.4→4.8，8.0→5.1，8.1/8.2/8.3→6.0，8.4+→6.2 |
| xdebug | 3.1 / 3.5 | 7.4→3.1，8.x→3.5 |
| libcurl | 7.88 / 8.23 | 8.23.0；7.4 / 8.0 用 7.88（配 openssl 1.1.1） |
| libxml2 | 2.11 / 2.15 | 2.15.4；7.4 / 8.0 用 2.11（2.12 起 PHP 8.0 编不过） |
| nghttp2 | 1.64 | 1.64.0 |
| zlib | 1.3 | 1.3.2（PHP 8.5 gd 要求 >= 1.2.11） |
| libpng | 1.6 | 1.6.59 |
| jpeg | 3.1 | 3.1.4.1（libjpeg-turbo） |
| webp | 1.6 | 1.6.0（PHP 8.5 gd 要求 >= 0.2.0） |
| aom | 3.12 | 3.12.1 |
| avif | 1.3 | 1.3.0（PHP 8.5 gd 要求 >= 0.8.2） |
| freetype | 2.13 | 2.13.3 |
| sqlite | 3.54 | 3.54.1 |
| libzip | 1.12 | 1.12.1 |
| libsodium | 1.0 | 1.0.22 |
| oniguruma | 6.9 | 6.9.10 |
| redis | 6.3 | 6.3.0（PECL） |
| apcu | 5.1 | 5.1.28（PECL） |
| igbinary | 3.2 | 3.2.16（PECL） |
| msgpack | 3.0 | 3.0.1（PECL） |
| protobuf | 5.36 | 5.36.2（PECL，最低 PHP 8.2） |
| yaml | 2.3 | 2.3.0（PECL） |
| memcached | 3.4 | 3.4.0（PECL） |
| imagick | 3.8 | 3.8.1（PECL） |
| ssh2 | 1.5 | 1.5.0（PECL，最低 PHP 8.0） |
| amqp | 2.2 | 2.2.0（PECL） |
| rdkafka | 6.0 | 6.0.5（PECL） |
| libyaml | 0.2 | 0.2.5 |
| libmemcached | 1.1 | 1.1.4 |
| imagemagick | 7.1 | 7.1.2-35 |
| libssh2 | 1.11 | 1.11.1 |
| rabbitmq-c | 0.18 | 0.18.1 |
| librdkafka | 2.16 | 2.16.0 |

## 状态文件

`install` / `update` / `add` / `remove` 写 `.phprt/<产品>-<系列>/state.json`；
`plan` / `list` 只读。`install --component` 只影响本次构建，不把临时组件写进 state。

状态记录：

- 构建方案（哪些组件、mode、installed）
- 显式指定的版本线 `bindings`
- 每个组件/依赖实际使用的线和小版本
- dep 的 feature 并集

bundle 默认线不写进 `bindings`；显式 `version:` 会写入并保留。默认组件集合只在首次写
state 时 materialize；`update` 只在已记录的线内更新补丁。详见
[docs/STATE.md](docs/STATE.md)。

## 已做的校验

- 每个命令只接受一个 `产品@系列`；目标必须带 `@`。
- 组件声明 `LINES` 时，线集合必须和 catalog 一致；dep 不允许声明
  `LINES`/`PRODUCT_LINES`/`PRODUCT_DEP_LINES`。
- `PRODUCT_LINES` / `PRODUCT_DEP_LINES` 引用的产品系列和线必须存在于 catalog。
- 每条线必须支持当前产品系列；显式线不满足直接报错。
- feature 的 `requires.commands` 会在选择 / state 校验时检查 PATH；缺命令时显式选择报错，
  bundle 默认丢弃并提示。
- 产品 profile 的 `tools` 和本次闭包内节点的 `host_tools` 在下载前检查：缺命令报
  `缺少构建工具`，版本不够报 `需要 <命令> >= <最低版本>，当前 <实际版本>`；并发声明时取最严。
- 安装后动作的路径必须是安装树内的相对路径，拒绝绝对路径、`..` 和空路径；没有任意 shell。
- `copy_ca_bundle` 找不到系统 CA 时只警告，不中断安装。
- 依赖线取交集；显式 pin 冲突报错，bundle 软默认只在唯一候选时自动切换。
- 版本线不存在、组件没有版本表、`version:` 写成补丁号都会报错。
- 组件契约：`modes` 非空且默认 mode 在 `modes` 里；external 必须有 `url`+`version`；
  bundled 必须有产品 `bundled` 条目；mode 必须由产品 profile 定义。
- 产品 recipe 的 `bundled` 无重复；`bundled_disable` 的 key 必须是 bundled 组件，
  值必须是单个 `--without-*` / `--disable-*`；profile 的 `strategy` 合法。
- bundle `[default_components]` 是表（`mode` / `features` / `deps`）；名字必须是 bundled
  组件且有 recipe，不允许 `version:`；条件 feature 不满足时提示并丢弃。
- `[component_presets]` 只提供选中组件时的默认配置；`default_components` 的 `ref` 必须是
  `"component_presets.<同名组件>"`，不能和本地 `features`/`deps` 同时写。
- `dep_configure` 的 key 必须在 `needs` 或某个 feature 的 `needs` 里；方括号里的 feature
  必须在该 dep 的 `features` 里。
- 组件的 `features` 只允许 `needs` / `configure` / `requires`；`requires` 不满足时显式
  选择直接报错。
- `audit` 对照上游选项清单：组件有未处置的选项、或 `ignored` 写了不存在的选项时退出码 1；
  处置规则和默认值判据见 [docs/FEATURES.md](docs/FEATURES.md)。
- 下载缓存命中跳过；有 sha256 的文件按哈希判断，没有 sha256 时按远端大小判断。
- `--downloads` 只下载，不解压、不编译、不写状态；下载、解压或编译失败都不写状态。
- 解压目标 `build/<名字>/<补丁>/` 已存在时跳过。
- 编译按节点签名跳过已完成的部分；签名变化先 `make distclean`，CMake 依赖再清
  `CMakeCache.txt` / `CMakeFiles`。
- PHP 编译结束写 `myphp-<系列>`（只有一个 PHP 系列时另写 `myphp`），并校验
  `php -v` 和 `php -m`；nginx 校验 `nginx -V`。
- 目标名重复、未知目标、裸名歧义、依赖成环都报错。

## 还没做

- 在线更新 catalog（目前是文件）。
- 精确补丁 pin / lock 文件。
- 项目清单、当前产品、默认版本。
- 一条命令安装多个产品；跨产品共享安装树。
- CPython 产品（设计见 [docs/GENERIC.md](docs/GENERIC.md)，当前只落地 PHP 和 nginx）。
- `--static-all`、多 PHP 同时构建、LTO。
- 按工具版本自动生成 CFLAGS；`post_install` 里执行任意 shell。
