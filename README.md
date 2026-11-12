# phprtv3

phprt v3 的最小原型：**Starlark 写 recipe，Rust 做求解和模拟执行**。

当前只验证架构，不下载、不编译。`plan` 算出依赖闭包、拓扑顺序和每个节点的参数；
`install` / `update` / `ext add` / `ext remove` 是模拟命令，只写状态文件。

设计细节见 [docs/DESIGN.md](docs/DESIGN.md)。

## 快速开始

```bash
cargo run -- list        # 列出 recipe
cargo run -- graph       # 打印依赖边
cargo run -- plan        # 求解 php 的全部依赖并模拟构建
cargo run -- plan php:8.5.12 --json
cargo run -- install --ext curl=sub:curl[nghttp2]   # 写状态文件
cargo run -- ext add xdebug
cargo run -- update
```

按扩展选择链接方式和子功能：

```bash
cargo run -- plan --ext swoole=sub:curl[ssl,nghttp2] --ext curl=link:shared,sub:curl[ssl]
cargo run -- plan --ext openssl,curl=link:shared
```

目标是位置参数，写在 `--ext` 前面：

```bash
cargo run -- plan php:8.5.12 --ext openssl=link:shared
```

契约不允许的组合会在解析阶段直接失败，不输出计划：

```bash
cargo run -- plan --ext xdebug=link:static
# error: ext:xdebug 契约不支持 static（支持：shared）
```

输出节选（`cargo run -- plan --ext swoole=sub:curl[ssl] --ext curl=link:shared,sub:curl[nghttp2]`）：

```text
build order
   1. dep:openssl     3.5.4
   2. ext:openssl     (builtin static)
   3. php:8.5.12
   4. dep:nghttp2     1.64.0
   5. dep:libcurl     8.11.1
   6. ext:curl        (builtin shared)
   7. ext:swoole      (pecl shared)
   8. ext:xdebug      (pecl shared)

[5/8] dep:libcurl 8.11.1
  features   nghttp2, ssl
  needs      dep:nghttp2, dep:openssl
  configure  ./configure --prefix=...
  configure  --with-nghttp2=.../deps/nghttp2-1.64.0
  configure  --with-openssl=.../deps/openssl-3.5.4
```

## 契约

`recipes/defs.star` 提供 `dep` / `ext` / `php` / `feature` / `sub`，recipe 文件在顶层定义 `TARGET`：

```python
TARGET = dep(name, version, url, sha256="", needs=[], features={},
             configure=[], make=["make"], artifacts=[])
TARGET = ext(name, origin, modules, link, version="", needs=[], subs={},
             configure=[], shared_from="", url="", sha256="", ini="", ini_lines=[])
TARGET = php(version, url, exts=[], configure=[], make=[...], artifacts=["bin/php"])
```

- `dep`：独立编译并安装到 `.phprt/php-<版本>/deps/<name>-<version>/`。
- `ext`：`origin=builtin` 默认 static，`origin=pecl` 默认 shared；
  `link` 是契约，只允许这里列出的链接方式。
- `php`：PHP 本体，`exts` 里的扩展参与依赖求解。
- `needs` 只能指向 `dep`，`exts` 只能指向 `ext`。同名不会混淆：
  依赖是 `dep:openssl`，扩展是 `ext:openssl`。

### 链接方式

| link | 做什么 | ini |
| --- | --- | --- |
| `static` | `configure` 参数进 PHP 的 `./configure`，编进 `bin/php` | 不需要 |
| `shared` | 编成 `.so`：builtin 从 PHP 源码的 `shared_from` 拷出来，PECL 用 `url` 下载源码；然后 `phpize` + `./configure --with-php-config=...` + `make install` | `etc/conf.d/<ini>` |

求解顺序：static 扩展在 php 之前，shared 扩展在 php 之后（它要用 `phpize` 和
PHP 源码/`php-config`）。

### 子配置与 feature

`sub:` 打开的是**扩展自己的子配置**，方括号里才是转给依赖的 feature：

```python
# ext/swoole.star：sub:curl 才启用 --enable-swoole-curl
subs = {
    "curl": sub(dep="libcurl", configure=["--enable-swoole-curl"]),
}

# ext/curl.star：开关在顶层 configure，sub:curl 只负责转发
subs = {"curl": sub(dep="libcurl")}

# dep/libcurl.star：feature 怎么编由 dep 决定
features = {
    "ssl": feature(needs=["openssl"], configure=["--with-openssl={dep:openssl}"]),
    "nghttp2": feature(needs=["nghttp2"], configure=["--with-nghttp2={dep:nghttp2}"]),
}
```

`--ext swoole=sub:curl[ssl,nghttp2]` 会给 swoole 加 `--enable-swoole-curl`，同时把
`ssl`、`nghttp2` 转给 libcurl；不写 `sub:curl` 则两者都不发生。

### 命令行语法

```
--ext <spec>[,<spec>...]        # 可重复，每次一个 spec
spec := 名字 | 名字=link:模式 | 名字:link:模式
      | 名字=link:模式,sub:子配置[feature,feature]
      | 名字=sub:子配置[feature,feature]
```

- `--ext` 可以写多次，每次一个 spec。
- 逗号既分隔扩展，也分隔同一个扩展的选项；`link:` / `sub:` 开头的片段并入前一个扩展。
- 名字后面的 `=` 和 `:` 等价，例如 `curl=link:shared` 和 `swoole:link:shared`。
- 裸名字用默认值：builtin → static，pecl → shared。
- 只支持 `link:` / `sub:` 选项，不再有 `名字=shared` 这种简写。
- `sub:名字` 打开扩展自己的子配置；方括号里的 feature 必须存在于该子配置绑定的 dep。
- `[...]` 里的逗号不参与外层切分；不写方括号就只开开关、不转发 feature。

### 状态文件

`install` / `update` / `ext add` / `ext remove` 写 `.phprt/php-<系列>/state.json`
（例如 `.phprt/php-8.5/state.json`），`plan` 只读。状态记录构建方案、installed 标记、
每个扩展的子配置和 dep 的 feature。详见 [docs/STATE.md](docs/STATE.md)。

字符串占位符：

| 占位符 | 含义 |
| --- | --- |
| `{tree}` | `.phprt/php-<php 版本>` |
| `{deps}` | `{tree}/deps` |
| `{build}` | 当前节点的解压目录 |
| `{prefix}` | 当前节点的安装前缀 |
| `{dep:名字}` | 指定依赖的安装前缀，如 `{dep:openssl}` |

## 当前 recipe

```text
php8.5 ──┬── ext:openssl ── dep:openssl
         ├── ext:curl    ── dep:libcurl ── (feature nghttp2) dep:nghttp2
         ├── ext:swoole  ── dep:libcurl  (pecl, shared only)
         └── ext:xdebug  (pecl, shared only)
```

| 文件 | 目标 |
| --- | --- |
| `recipes/php8.5.star` | `php:8.5.12` |
| `recipes/ext/openssl.star` | `ext:openssl`，link=static,shared |
| `recipes/ext/curl.star` | `ext:curl`，link=static,shared，subs 转给 libcurl |
| `recipes/ext/swoole.star` | `ext:swoole` 6.0.2，link=shared |
| `recipes/ext/xdebug.star` | `ext:xdebug` 3.4.7，link=shared，`zend_extension` |
| `recipes/dep/openssl.star` | `dep:openssl` 3.5.4 |
| `recipes/dep/libcurl.star` | `dep:libcurl` 8.11.1，features=ssl,nghttp2 |
| `recipes/dep/nghttp2.star` | `dep:nghttp2` 1.64.0 |

## 已做的校验

- ext 契约：`origin` 只能是 builtin/pecl；`link` 非空且默认值必须在 `link` 里；
  shared 必须有 `ini`，builtin 还要 `shared_from`，PECL 还要 `url`+`version`。
- `subs.*.dep` 必须出现在 `needs` 里；方括号里的 feature 必须在该 dep 的 `features` 里。
- `state.json` 里的扩展和 feature 必须在 recipe 里存在，否则报错。
- 请求契约不支持的 link 直接报错，例如 `xdebug=link:static`、`swoole=link:static`。
- `needs` / `exts` / feature 的依赖必须存在。
- 目标名重复、未知目标、裸名歧义、依赖成环都报错。
- `cargo test` 覆盖默认模式、显式模式、PECL 拒绝 static、子配置与 feature 传播、
  feature 并集不缩小、未知子配置、成环、缺依赖、歧义名和占位符展开。

## 还没做

- 下载、解压、真编译，以及安装树落盘。
- `--static-all`（把所有扩展和依赖静态链进一个二进制，不含 libc）。
- shared 扩展真实构建后 `.so` 的路径发现（`php-config --extension-dir`）。
- 构建锁、签名、增量构建、`verify`、可移动性检查。
- 多版本 bundle、LTO。
