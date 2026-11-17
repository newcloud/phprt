# 添加或更新 recipe

本文件说明新增产品、版本线、组件、升级补丁号、更换下载地址时怎么改，以及 catalog
下载地址的判定规则。字段含义见 [CONTRACT.md](CONTRACT.md)，版本数据规则见 `AGENTS.md`
第 4 节。

## 1. 改哪里

| 改什么 | 动哪些文件 |
| --- | --- |
| 升补丁号、换下载地址 | `recipes/catalog.toml` |
| 新增版本线 | `recipes/catalog.toml` + 消费方的 `LINES` / `PRODUCT_LINES` / `PRODUCT_DEP_LINES`，必要时 `recipes/bundles/<产品>/<系列>.toml` |
| 换默认线 | `recipes/bundles/<产品>/<系列>.toml` |
| 新增组件 / 改构建参数 | `recipes/components/<产品>/<名字>.star` |
| 新增 bundled 组件事实 | `recipes/products/<产品>/<系列>.star` 的 `bundled` / `bundled_disable` |
| 默认启用的组件集合 | `recipes/bundles/<产品>/<系列>.toml` 的 `default_components` |
| 新产品 | `recipes/catalog.toml` + `recipes/products/<产品>/_profile.star` + `recipes/products/<产品>/<系列>.star` + `recipes/bundles/<产品>/<系列>.toml` |

补丁号只写在 `recipes/catalog.toml`。recipe、bundle、state 都不写补丁号。

## 2. 先取真实下载地址

catalog 里的 `url` 必须是上游实际提供文件的地址。先查候选地址是否跳转：

```bash
curl -sSI --max-time 15 https://www.openssl.org/source/openssl-3.5.9.tar.gz | grep -iE '^(HTTP/|location:)'
```

输出示例：

```text
HTTP/2 301
location: https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz
```

## 3. 跳转怎么判定，怎么写

| 情况 | 写法 | 例子 |
| --- | --- | --- |
| 跳到另一个站点 | 写**跳转后的最终地址**，并加注释说明从哪跳来 | `openssl.org` → `github.com` |
| 同一发布方的临时签名或 CDN 跳转 | 写稳定的发布地址，不写带签名参数的临时地址 | `github.com` → `release-assets.githubusercontent.com` |
| 不跳转 | 写原地址 | `php.net`、`pecl.php.net`、`curl.se`、`nginx.org` |

跳转后换站点的地址必须写成最终地址，原因是下载器按 catalog 里的原始主机做三件事：

- 只有 `https://github.com/` 开头才追加镜像（`crates/download/src/candidate.rs`）
- GitHub 直连用 30 秒短超时，失败后尽快换镜像
- 站点并发按主机分桶，GitHub 主站和镜像共用一个桶

写成会跳转的旧地址时，这三条规则都按旧主机判断，等于全部失效。

注释写清跳转关系，便于下次升级时核对。示例：

```toml
# OpenSSL 官方发布页只做跳转：https://www.openssl.org/source/openssl-<版本>.tar.gz
# 会 301 到 GitHub Releases，所以这里直接写跳转后的最终地址。
[openssl]
"3.5" = { version = "3.5.9", url = "https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz", sha256 = "603f5602..." }
```

规则：

- 已有注释不删除。升级版本、换地址时同步更新注释内容，保留原意。
- 注释只解释「为什么是这个地址」，不复述版本号。

## 4. 填 sha256

有官方校验值时写入 `sha256`。没有时先下载再计算：

```bash
aria2c -x16 -s16 -d /tmp/opencode -o openssl-3.5.9.tar.gz https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz
sha256sum /tmp/opencode/openssl-3.5.9.tar.gz
```

- `sha256` 用于缓存校验，也用于下载完成后的校验。
- 没有 `sha256` 时只能按远端大小校验，见 [DOWNLOAD.md](DOWNLOAD.md) 第 7 节。

## 4.1 登记旧补丁

需要装指定旧补丁（降级、测试）时，在 catalog 的线里加 `patches`：

```toml
"8.4" = { version = "8.4.14", url = ".../php-8.4.14.tar.gz", patches = [
    { version = "8.4.13", url = ".../php-8.4.13.tar.gz" },
] }
```

- `version` / `url` / `sha256` 仍是当前最新补丁；
- `patches` 只放旧补丁，不能和当前补丁重复；
- 不写 `patches` 时 `install php@8.4.13` 会报错并列出可用补丁。

## 5. 新增版本线

除 catalog 外，还要同步：

- 有自己版本线的组件：`recipes/components/<产品>/<名字>.star` 的 `LINES`，
  线集合必须和 catalog 一致
- bundled 组件：`recipes/components/<产品>/<名字>.star` 的 `PRODUCT_LINES`（同名依赖）
  或 `PRODUCT_DEP_LINES`（不同名依赖，如 gd 的 libpng/jpeg）
- 产品自己对依赖的约束：`recipes/products/<产品>/<系列>.star` 的 `LINES`
- 默认线变化：`recipes/bundles/<产品>/<系列>.toml`
- 新增产品系列：`recipes/products/<产品>/<系列>.star` +
  `recipes/bundles/<产品>/<系列>.toml` + catalog 里该产品的线

## 6. 添加产品

### 6.1 产品 recipe

路径是 `recipes/products/<产品>/<系列>.star`。`TARGET` 必须是 `product(...)` dict。

产品级共享 profile 放在 `recipes/products/<产品>/_profile.star`，只定义 `PROFILE`；
系列 recipe 用 `profile = PROFILE`。需要按系列改工具或编译参数时用
`extend_profile(PROFILE, ...)`，规则见 [PRODUCT.md](PRODUCT.md)。

```python
TARGET = product(
    name = "nginx",
    series = "1.29",
    needs = ["pcre2", "openssl"],
    configure = ["./configure", "--prefix={tree}", ...],
    make = ["make", "make install"],
    artifacts = ["sbin/nginx"],
    profile = PROFILE,
    desc = "nginx 1.29 本体",
)
```

- `name` 必须和目录名一致，`series` 必须和文件名一致。
- `version` / `url` / `sha256` 由 catalog 注入，不要写死。
- `bundled` / `bundled_disable` 是“产品源码包里自带哪些组件、没选中时怎么关闭”。
- `profile` 定义组件怎么挂到产品上，见 [CONTRACT.md](CONTRACT.md) 第 6 节。
- `_profile.star` 里的 `tools` 声明宿主机命令和最低版本；`post_install` 声明安装后
  写哪些文件、复制 CA、写组件 ini。
- 产品自己的 `LINES[系列]` 可以约束依赖线。

### 6.2 组件 recipe

路径是 `recipes/components/<产品>/<名字>.star`：

```python
def target(ctx):
    self = ctx["self"]
    return component(
        name = "lua-nginx-module",
        product = "nginx",
        source = "external",
        modes = ["dynamic"],
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = ["luajit"],
        env = {
            "LUAJIT_INC": "{dep:luajit}/include/luajit-2.1",
            "LUAJIT_LIB": "{dep:luajit}/lib",
        },
        desc = "OpenResty lua-nginx-module，动态模块",
    )

TARGET = target
```

- `product` 必填；`source` 是 `bundled` / `external`。
- bundled 组件由产品 recipe 的 `bundled` 列表推导；`source_path` 覆盖产品 profile 的
  `bundled_path`（PHP 默认 `ext/{name}`）。
- external 组件必须有 catalog 的 url/version。
- `modes` 必须在该产品 profile 的 `modes` 里。
- 组件自己的开关写 `features`；有条件的能力用 `requires` 声明，不要写成
  `target(ctx)` 里的 `if`。
- 需要宿主机命令的 feature 用 `requires.commands`，不要塞进 `needs`。
- 上游源码需要先改再编时，用 `prepare` 写 shell 命令；命令在 standalone 构建前于源码
  目录执行，幂等（重复执行无副作用），会进构建签名。
- 依赖的集成参数写 `dep_configure`，key 是 `needs` 里的依赖名，或某个 feature 的
  `needs` 里的条件依赖。

### 6.3 PHP 版本事实

新增或适配一个 PHP 版本时，在 `recipes/products/php/<系列>.star` 写两组常量：

```python
BUNDLED = ["openssl", "curl", "libxml", "dom", ...]

BUNDLED_DISABLE = {
    "openssl": "--without-openssl",
    "libxml": "--without-libxml",
    "dom": "--disable-dom",
    ...
}
```

规则：

- `bundled` 从该 PHP 版本的 `configure --help` / 源码 `ext/` 查出来，固定成常量；
  不管当前有没有组件 recipe。
- `bundled_disable` 是每个 bundled 组件的关闭参数，一个组件一个参数，允许写成字符串，
  不允许列表或空格。
- 只列 phprt 需要控制的 bundled 组件；`--enable-filter` 这类基础参数不放进表里。
- 未适配的 PHP 系列先写最小 `bundled` 和 `bundled_disable`，等适配时再补全。

### 6.4 默认启用

默认集合写在 `recipes/bundles/<产品>/<系列>.toml` 的 `[default_components]` 表：

```toml
[component_presets]
openssl = { features = ["argon2"] }
curl = { deps = { libcurl = ["ssl"] } }
gd = { features = ["jpeg", "webp", "freetype"] }

[default_components]
openssl = { ref = "component_presets.openssl" }
curl = { ref = "component_presets.curl" }
libxml = {}
```

规则：

- `[component_presets]` 提供“组件被选中时”的默认配置，不决定是否默认安装。
- `[default_components]` 决定默认安装集合，必须放在文件最后。
- `[default_components]` 可以用 `ref = "component_presets.<同名组件>"` 引用预设；
  使用 `ref` 时不能再写本地 `features`/`deps`；`mode` 可以本地覆盖。
- 不写 `ref` 时只使用本地配置，不会隐式套同名 preset。
- key 是组件名；每个组件可写 `mode` / `features` / `deps`，都不允许 `version:`。
- 只有产品 `bundled` 列表里的组件、并且已有 component recipe，才能进
  `default_components`。
- 有依赖的 bundled 组件必须和依赖一起写，要转给依赖的 feature 显式列在 `deps` 里。
- 不做依赖展开、不做依赖校验；漏写时产品 configure 会报错。
- 未适配的产品系列不写 `[default_components]`，等价于空表。
- 条件 feature 在当前产品 / 依赖线下不可用时提示并丢弃。

### 6.5 PHP 8.5 当前默认

```toml
product = "php"
swoole = "6.2"
xdebug = "3.5"
redis = "6.3"
apcu = "5.1"
igbinary = "3.2"
msgpack = "3.0"
protobuf = "5.36"
yaml = "2.3"
memcached = "3.4"
imagick = "3.8"
ssh2 = "1.5"
amqp = "2.2"
rdkafka = "6.0"

[deps]
openssl = "3.5"
zlib = "1.3"
libpng = "1.6"
jpeg = "3.1"
webp = "1.6"
aom = "3.12"
avif = "1.3"
freetype = "2.13"
libyaml = "0.2"
libmemcached = "1.1"
imagemagick = "7.1"
libssh2 = "1.11"
rabbitmq-c = "0.18"
librdkafka = "2.16"
sqlite = "3.54"
libzip = "1.12"
libsodium = "1.0"
oniguruma = "6.9"

[component_presets]
openssl = { features = ["argon2"] }
curl = { deps = { libcurl = ["ssl"] } }
gd = { features = ["jpeg", "webp", "freetype"] }
zip = { deps = { libzip = ["encryption"] } }
sqlite3 = { deps = { sqlite = ["column_metadata"] } }
pdo_sqlite = { deps = { sqlite = ["column_metadata"] } }

[default_components]
openssl = { ref = "component_presets.openssl" }
curl = { ref = "component_presets.curl" }
libxml = {}
zlib = {}
sqlite3 = { ref = "component_presets.sqlite3" }
pdo_sqlite = { ref = "component_presets.pdo_sqlite" }
mysqli = {}
pdo_mysql = {}
mbstring = {}
zip = { ref = "component_presets.zip" }
sodium = {}
gettext = {}
pcntl = {}
sockets = {}
shmop = {}
sysvmsg = {}
sysvsem = {}
sysvshm = {}
```

顶层是组件默认线，`[deps]` 是依赖库默认线；bundled 组件没有版本线，不能写顶层。
`swoole` / `xdebug` / `gd` / 11 个 PECL 扩展不在默认集里，需要 `add` 或 `--component`
显式选择；openssl、curl、gd 选中时自动使用各自的 `[component_presets]`。
默认集和 preset 都只在第一次写 state 时落盘。

## 7. 验收

```bash
cargo fmt --all
cargo test --workspace
cargo run -- install php@8.5 --downloads
```

`--downloads` 会按 catalog 地址真实联网下载。网络受限时先设代理：

```bash
cargo run -- install php@8.5 --downloads --proxy http://127.0.0.1:10808
```

下载完成后再跑完整安装：

```bash
cargo run -- install php@8.5
```
