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
| 新增可下载组件（dep / 组件 / 工具） | `recipes/catalog.toml` + 对应的 `recipes/<类别>/<名字>.star` + `recipes/upstream/<名字>.star`（漏了 `cargo test -p phprt-upstream` 会失败） |
| 新增 bundled 组件事实 | `recipes/products/<产品>/<系列>.star` 的 `bundled` / `bundled_disable` |
| 默认启用的组件集合 | `recipes/bundles/<产品>/<系列>.toml` 的 `default_components` |
| 新产品 | `recipes/catalog.toml` + `recipes/products/<产品>/_profile.star` + `recipes/products/<产品>/<系列>.star` + `recipes/bundles/<产品>/<系列>.toml` |
| 新增 / 升级构建期工具 | `recipes/catalog.toml` + `recipes/tools/<名字>.star` |
| 编译器版本档位 | 产品系列 recipe 的 `TOOLCHAIN` |

补丁号只写在 `recipes/catalog.toml`。recipe、bundle 不写补丁号；state 只记录实际装过的
`line` / `version`，作为 `--lock` 的版本来源，源码仍从 catalog 取（见 [LOCK.md](LOCK.md)）。

## 2. 版本声明原则

只声明**我们主动适配**的版本：有兼容性判断、补丁、构建参数差异的线。

- 默认用最新、ABI 兼容的依赖不逐系列重复声明；不写就是沿用 bundle 的 `[deps]`
  默认线，或约束剩下的唯一可用线。
- 例外必须写：某个系列与某条线不兼容（如 PHP 7.4/8.0 不能用 libxml2 2.12+，
  也不能用需要 OpenSSL 3.x 的 libcurl 8.23），写进组件或产品的约束，并注释原因。
- 主线默认只写一处：bundle 的 `[deps]`。组件约束只写“允许/不允许”的事实，
  不重复默认值。
- 看到 `注意: xxx 自动使用唯一可用线` 时：说明没有默认线，只是约束恰好只剩一条。
  要么在 bundle 里写明确默认（推荐），要么把约束收敛成只写例外。

### 2.1 什么时候新增版本线

新增一条线之前先回答四个问题，四个都说“是”才值得加：

1. **产品需要吗**：这个产品的构建或运行真的需要它（上游要求、修 bug、安全更新、依赖联动）。
2. **有必要主动适配最新吗**：最新版是我们想主动支持的目标，而不是“上游发了就加”。
   不追每个新版本；当前主线稳定就先用主线。
3. **产品支持吗**：recipe 的 `LINES` / 兼容矩阵允许这个系列用这条线，能编过、能跑。
4. **有特殊收益吗**：新功能、新 API、性能优化，或者修掉了当前主线上的问题。

四条都不成立就不加。不加不是缺漏，是刻意选择；“能编过”不等于“值得加”。

加线之后要：写 catalog 地址、跑 `phprt audit`、走完整构建验收，并在注释里写清适配理由。

给 agent 的边界：

- 不能拿“默认最新”当借口跳过已知的不兼容、构建失败或行为差异。
- 不能为了“可配置”新增未验证的线；每条线都要有 catalog 地址并通过构建。
- 不要把同一个默认线在每个系列里重复一遍，也不要靠约束恰好剩一条来隐式选线。

## 3. 先取真实下载地址

catalog 里的 `url` 必须是上游实际提供文件的地址。先查候选地址是否跳转：

```bash
curl -sSI --max-time 15 https://www.openssl.org/source/openssl-3.5.9.tar.gz | grep -iE '^(HTTP/|location:)'
```

输出示例：

```text
HTTP/2 301
location: https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz
```

## 4. 跳转怎么判定，怎么写

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

## 5. 填 sha256

有官方校验值时写入 `sha256`。没有时先下载再计算：

```bash
aria2c -x16 -s16 -d /tmp/opencode -o openssl-3.5.9.tar.gz https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz
sha256sum /tmp/opencode/openssl-3.5.9.tar.gz
```

- `sha256` 用于缓存校验，也用于下载完成后的校验。
- 没有 `sha256` 时只能按远端大小校验，见 [DOWNLOAD.md](DOWNLOAD.md) 第 7 节。

## 5.1 一条线的补丁数组

每条线是一个数组，保存这条线发布过的所有补丁。默认选择取数组中**版本最大**的
那个，和数组顺序无关；新补丁直接加进数组，旧补丁保留不删。
适用于产品、组件和依赖库的所有线：

```toml
"8.4" = [
    { version = "8.4.14", url = ".../php-8.4.14.tar.gz", sha256 = "..." },
    { version = "8.4.13", url = ".../php-8.4.13.tar.gz", sha256 = "..." },
]
```

- 每个元素的 `url` 必须是该版本的真实地址；有 `sha256` 时要能通过校验；
- 版本号在同一条线里不能重复；
- 显式安装旧补丁、`--lock`、`scheme --lock` 都按版本在数组里精确查找；
  找不到就报错，不会改用最大版本（见 [LOCK.md](LOCK.md)）。

少了旧版本时 `install php@8.4.13` 报错并列出该线可用的版本。

## 6. 新增版本线

除 catalog 外，还要同步：

- 有自己版本线的组件：`recipes/components/<产品>/<名字>.star` 的 `LINES`，
  线集合必须和 catalog 一致
- bundled 组件：`recipes/components/<产品>/<名字>.star` 的 `PRODUCT_LINES`（同名依赖）
  或 `PRODUCT_DEP_LINES`（不同名依赖，如 gd 的 libpng/jpeg）
- 产品自己对依赖的约束：`recipes/products/<产品>/<系列>.star` 的 `LINES`
- 默认线变化：`recipes/bundles/<产品>/<系列>.toml`
- 新增产品系列：`recipes/products/<产品>/<系列>.star` +
  `recipes/bundles/<产品>/<系列>.toml` + catalog 里该产品的线

## 7. 添加产品

### 7.1 产品 recipe

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
- `_profile.star` 里的 `tools` 声明构建期工具和版本区间；gcc / g++ 不在 `tools` 里，
  系列 recipe 用 `toolchain = TOOLCHAIN` 声明编译器档位。`post_install` 声明安装后
  写哪些文件、复制 CA、写组件 ini。
- 产品自己的 `LINES[系列]` 可以约束依赖线。

#### 构建期工具 tools

工具分两类：

- 宿主机命令（make / perl / patchelf / m4 / cmake / nasm）：直接用 `tool(...)` 声明，
  不需要 catalog 记录；
- 可从源码构建的工具（目前只有 autoconf）：在 `recipes/tools/<名字>.star` 写
  `tool_target(...)`，并在 `recipes/catalog.toml` 里给同名条目。

```python
# recipes/tools/autoconf.star
def target(ctx):
    self = ctx["self"]
    return tool_target(
        name = "autoconf",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        tools = [tool("m4", min = "1.4")],
        configure = ["./configure", "--prefix={prefix}"],
        make = ["make", "make install"],
        artifacts = ["bin/autoconf"],
        desc = "构建期工具：phpize 需要 autoconf",
    )

TARGET = target
```

- 工具装到 `build/tools/<名字>-<版本>`，跨产品复用；删 `build/` 会重编。
- 工具不能声明 `needs`，只能声明 `tools`（工具依赖工具）。
- 编译器（`gcc` / `g++` / `gxx` / `cc` / `c++`）不能做成 Tool recipe，也不能写进
  `tools`；它们只属于产品 `toolchain`。
- catalog 里保留旧版本：一条线就是一个可选版本；节点可以用 `max` 收窄到旧版，
  同一次构建允许同名工具多版本并存。需要精确复现时 `--lock` 从 `state.tools` 取
  built 版本。
- 解析规则、签名和 `--downloads` 行为见 [TOOLS.md](TOOLS.md) 第 3 节。

#### 编译器档位 toolchain

产品需要限制或适配编译器时，在系列 recipe 里写 `TOOLCHAIN`：

```python
TOOLCHAIN = toolchain(
    gcc = [
        toolchain_range(min = "9", max = "13"),
        toolchain_range(min = "14", cflags = "-std=gnu17 -Wno-error=..."),
    ],
    gxx = [toolchain_range(min = "9")],
)

TARGET = product(name = "php", ..., toolchain = TOOLCHAIN)
```

- 档位不能重叠；`gcc` 档位只写 `cflags`，`gxx` 档位只写 `cxxflags`。
- 编译器太新需要兼容参数时加档位，不要用 `max` 把新编译器拒掉。
- 新系列要自己写完整的 `toolchain`，不要放进 `_profile.star`。

#### 构建目标 targets

产品有多个构建目标（如 PHP 的 cli / fpm / cgi）时，在**每个系列自己的** recipe 里写
完整的 `targets = {...}`，不要放进 `_profile.star`，也不要做共享常量。版本差异直接
在该文件里体现；重复是允许的。

```python
TARGETS = {
    "cli": {
        "flags": ["--enable-cli"],
        "disable_flags": ["--disable-cli"],
        "artifacts": ["bin/php"],
    },
    "fpm": {
        "flags": ["--enable-fpm"],
        "disable_flags": [],   # 上游没有该参数时不要写
        "artifacts": ["sbin/php-fpm"],
        "config": {
            "user": {"type": "string", "flags": "--with-fpm-user={value}"},
        },
    },
}
TARGET = product(..., targets = TARGETS)
```

- 每个 target 至少写 `flags`；`disable_flags`、`config` 按实际上游能力写。
- 不要根据 `--enable-*` 推断存在 `--disable-*`。必须查该版本的 `configure --help`
  或 `configure.ac`；不支持的禁用参数就不写，未选中时不生成。
- 默认选择写在 `bundles/<产品>/<系列>.toml` 的 `[targets] default`，recipe 不写默认。
- 新增系列时同步检查：该系列声明的 target、bundle 默认值、target 的 configure 参数
  三者一致。`plan <产品>@<系列>` 和真实构建各跑一次。
- 规则见 [TARGETS.md](TARGETS.md)。

### 7.2 组件 recipe

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

### 7.3 PHP 版本事实

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

### 7.4 默认启用

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

### 7.5 PHP 8.5 当前默认

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

### 7.6 PHP 8.6 当前默认

PHP 8.6 还没有正式版，catalog 的 8.6 线指向 `php-8.6.0RC3`；正式版发布后追加
`8.6.0` 并指向 `www.php.net`。

```toml
product = "php"

[deps]
openssl = "3.5"
libcurl = "8.23"
libxml2 = "2.15"
zlib = "1.3"
libpng = "1.6"
jpeg = "3.1"
webp = "1.6"
aom = "3.12"
avif = "1.3"
freetype = "2.13"
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

和 8.5 的差别：

- 没有 PECL 扩展的默认线。PHP 8.6 删除了 `XtOffsetOf`、`INI_INT`、`zval_dtor` 等
  Zend API，swoole 6.2.5、xdebug 3.5.3 和 8.5 默认线上的 11 个 PECL 扩展（redis、
  apcu、igbinary、msgpack、protobuf、yaml、memcached、imagick、ssh2、amqp、rdkafka）
  的现有版本线都编不过，`LINES` 里也不写 `8.6`；等上游发支持 8.6 的版本再加回。
- gd 在 8.6 多了 `tiff` / `heif` / `imagequant` / `uhdr` 四个 feature，都不在预设里，
  需要显式打开。

## 8. 新增 recipe 的防错清单

loading 时会直接拒绝下列写法，不会静默忽略：

| 场景 | 规则 |
| --- | --- |
| 任何 `Target` | 只能写 contract 里的字段；未知字段直接报错（`host_tools` 已删除） |
| dep / component / tool 的 `tools` | 不能写编译器 `gcc` / `g++` / `gxx` / `cc` / `c++`；编译器只属于产品 `toolchain` |
| tool recipe | 必须在 `catalog.toml` 有同名条目；只能 `tool_target(...)`；不能有 `needs`、产品 / 组件字段 |
| toolchain | 至少声明 `gcc` / `gxx` 之一；档位不能重叠；`gcc` 只写 `cflags`、`gxx` 只写 `cxxflags` |
| component | `product` / `modes` / 默认 mode 必须和产品 profile 对得上；bundled 必须出现在产品 `bundled` 里 |
| bundle | 组件默认线必须在 `LINES[线].product` 里包含该系列，且线在 catalog 里；写错直接报错，不会自动改用唯一可用线 |
| dep | 必须有 version / url；不能写产品 / 组件字段、`prepare`、`dep_configure` |
| 产品系列 | 必须同时有 `products/<产品>/<系列>.star` 和 `bundles/<产品>/<系列>.toml`；catalog 系列集合要和 recipe 一致 |

新增后至少跑：

```bash
cargo run -- plan <产品>@<系列>
cargo run -- audit <产品>@<系列>
cargo test --workspace
```

`plan` 会打印本次解析到的工具（host / store）和编译器档位；工具或档位有问题会在
下载前报出来。

## 9. 验收

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
