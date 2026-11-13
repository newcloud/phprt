# phprtv3

phprt v3 的最小原型：**Starlark 写 recipe，Rust 做版本解析、求解、下载和构建**。

`plan` 算出依赖闭包、拓扑顺序和每个节点的参数。`install` / `update` 默认把方案里的
源码包下载到 `.phprt/downloads/`，按构建顺序解压到 `build/<组件>/<补丁>/`，编译安装到
`.phprt/php-<php 版本>/`，最后写状态并生成项目入口 `./myphp`。`--downloads` 只下载
源码，不解压、不编译、不写状态。

构建环境、日志、跳过规则和运行时库整理见 [docs/BUILD.md](docs/BUILD.md)。

版本和默认策略分成三层：

```text
bundle       php8.5 默认用 swoole 6.2、openssl 3.5；默认启用 openssl + curl
catalog      6.2 → 6.2.5 + 下载地址；3.5 → 3.5.9 + 下载地址
recipe       6.2 支持 PHP 8.4/8.5/8.6，需要 libcurl/openssl，怎么编
```

补丁更新只改 `recipes/catalog.toml`，bundle 和 recipe 不会过期。

设计细节见 [docs/DESIGN.md](docs/DESIGN.md)。

## 快速开始

```bash
cargo run -- install 8.5                 # 默认 openssl + curl(ssl) + PHP 8.5.12
./myphp -v                               # 用项目自己的 PHP
cargo run -- install 8.5 --disable-default-ext openssl --disable-default-ext curl  # 只装 PHP
cargo run -- install 8.5 --downloads     # 只下载源码包
cargo run -- install 8.5 --ext swoole --downloads   # 只下载扩展和依赖
cargo run -- install 8.5 --ext swoole --ext xdebug  # 再加两个 PECL 扩展
cargo run -- install 8.5 --ext swoole    # swoole 6.2.5 + libcurl + openssl 3.5.9
cargo run -- install 8.5 --ext curl=link:shared,sub:curl[ssl,nghttp2]
cargo run -- install 7.4 --ext swoole    # swoole 4.8.13 + openssl 1.1.1w
cargo run -- --php 8.5 ext list           # 查看扩展和版本
cargo run -- --php 8.5 ext add xdebug     # 加入方案（待安装）
cargo run -- --php 8.5 install 8.5        # 装方案里的扩展
cargo run -- --php 8.5 update             # 在记录的线内更新补丁
```

`install` 必须指定 PHP；不带时报错并给出示例：

```text
error: 请指定 PHP 版本，例如：phprt install 8.5（可用：7.4, 8.0, 8.2, 8.3, 8.4, 8.5, 8.6）
```

## 版本线

```bash
# 不写 version：用 php8.5 bundle 的默认线（swoole 6.2）
cargo run -- install 8.5 --ext swoole

# 显式选线：6.0.x 的最新补丁从 catalog 取
cargo run -- install 8.2 --ext swoole=version:6.0

# 只改依赖库的线，不额外安装扩展
cargo run -- install 8.5 --ext swoole --dep openssl=4.0

# 不支持当前 PHP 的线直接失败
cargo run -- install 8.5 --ext swoole=version:4.8
# error: swoole 4.8 不支持 PHP 8.5（可用：6.2）
```

`version:` 只接受线（`6.2`），不接受补丁（`6.2.5`）；补丁由 catalog 决定。

多个扩展约束同一个库时取交集。bundle 默认线冲突且只有一个候选时会自动切换并打印
提示；显式 `--dep` 冲突则直接报错。

## recipe

recipe 只声明版本关系、依赖和构建参数；版本号、下载地址从 ctx 注入：

```python
# recipes/ext/swoole.star
LINES = {
    "6.2": {
        "php": ["8.4", "8.5", "8.6"],
        "openssl": ["1.1.1", "3.0", "3.5", "4.0"],
    },
}

def target(ctx):
    self = ctx["self"]
    return ext(
        name="swoole",
        version=ctx["php_version"] if ctx["is_builtin"] else self["version"],
        url="" if ctx["is_builtin"] else self["url"],
        sha256="" if ctx["is_builtin"] else self["sha256"],
        modules=["swoole"],
        link=["shared"],
        needs=["libcurl", "openssl"],
        subs={"curl": sub(dep="libcurl", configure=["--enable-swoole-curl"])},
        configure=["--enable-swoole", "--with-openssl-dir={dep:openssl}"],
        ini="27-swoole.ini",
    )

TARGET = target
```

- `LINES`：扩展自己的线支持哪些 PHP、允许哪些依赖线。
- `PHP_LINES`：builtin 扩展的 PHP 系列 → 允许的依赖库线（如 ext:openssl）。
- dep recipe 不声明 PHP 关系；库版本由 bundle/CLI/消费扩展决定。
- `target(ctx)`：只负责构建参数，不再判断“能不能配”。
- `ctx["is_builtin"]`：由 php recipe 的 `builtin_exts` 推导，不在 ext recipe 里写 `origin`。
- `TARGET` 也可以是普通 dict。

### 子配置与 feature

`sub:` 打开扩展自己的子配置，方括号里的 feature 转给依赖：

```bash
cargo run -- install 8.2 --ext swoole=version:6.0,sub:curl[ssl,nghttp2]
```

- `sub:curl` 给 swoole 加 `--enable-swoole-curl`。
- `[ssl,nghttp2]` 转给 libcurl，具体编译参数由 libcurl 的 `features` 决定。
- 不写 `sub:curl` → 开关和转发都不发生。

### 占位符

| 占位符 | 含义 |
| --- | --- |
| `{tree}` | `.phprt/php-<php 版本>` |
| `{deps}` | `{tree}/deps` |
| `{build}` | 当前节点的解压目录 |
| `{prefix}` | 当前节点的安装前缀 |
| `{dep:名字}` | 指定依赖的安装前缀，如 `{dep:openssl}` |

## 当前版本表

PHP：7.4 / 8.0 / 8.2 / 8.3 / 8.4 / 8.5 / 8.6。

| 组件 | 线 | 默认使用 |
| --- | --- | --- |
| openssl | 1.1.1 / 3.0 / 3.5 / 4.0 | 8.5 用 3.5；4.0 允许但不默认 |
| swoole | 4.8 / 5.1 / 6.0 / 6.2 | 7.4→4.8，8.0→5.1，8.2/8.3→6.0，8.4+→6.2 |
| xdebug | 3.1 / 3.5 | 7.4→3.1，8.x→3.5 |
| libcurl | 8.23 | 8.23.0 |
| nghttp2 | 1.64 | 1.64.0 |

## 状态文件

`install` / `update` / `ext add` / `ext remove` 写
`.phprt/php-<系列>/state.json`，`plan` / `list` / `ext list` 只读。

状态记录：

- 构建方案（哪些扩展、link、installed）
- 显式指定的版本线 `bindings`
- 每个扩展/依赖实际使用的线和小版本
- dep 的 feature 并集

bundle 默认线不写进 `bindings`；显式 `version:` 会写入并保留。默认扩展集合只在
首次写 state 时 materialize；`update` 只在已记录的线内更新补丁。详见
[docs/STATE.md](docs/STATE.md)。

## 已做的校验

- `install` 必须指定 PHP；`--php` 与 install 参数冲突时报错。
- 扩展声明 `LINES` 时，线集合必须和 catalog 一致；dep 不允许声明 `LINES`/`PHP_LINES`。
- `PHP_LINES` 必须覆盖所有 PHP 系列，引用的线必须存在于 catalog。
- 每条线必须支持当前 PHP；显式线不满足直接报错。
- 依赖线取交集；显式 pin 冲突报错，bundle 软默认只在唯一候选时自动切换。
- 版本线不存在、组件没有版本表、`version:` 写成补丁号都会报错。
- ext 契约：`link` 非空且默认值必须在 `link` 里；shared 必须有 `ini`；
  内置扩展还要 `shared_from`，非内置还要 `url`+`version`；origin 由 `builtin_exts` 推导。
- php recipe 的 `builtin_exts` 非空无重复；`builtin_disable` 的 key 必须是内置扩展，
  值必须是单个 `--without-*` / `--disable-*`。
- bundle `default_exts` 是空格分隔的 spec；名字必须是内置扩展且有 recipe，不允许 `version:`。
- `subs.*.dep` 必须出现在 `needs` 里；方括号里的 feature 必须在该 dep 的 `features` 里。
- 下载缓存命中跳过；有 sha256 的文件按哈希判断，没有 sha256 时按远端大小判断。
- `--downloads` 只下载，不解压、不编译、不写状态；下载、解压或编译失败都不写状态。
- 解压目标 `build/<组件>/<补丁>/` 已存在时跳过。
- 编译按节点签名跳过已完成的部分；签名变化先 `make distclean`。
- 编译结束写 `myphp`、`.phprt/current`，并校验 `php -v` 和 `php -m`。
- 目标名重复、未知目标、裸名歧义、依赖成环都报错。

## 还没做

- 在线更新 catalog（目前是文件）。
- 精确补丁 pin / lock 文件。
- `--static-all`、多 PHP 同时构建、LTO。
- 除 8.5 以外 PHP 系列的默认扩展集和真构建验收。
