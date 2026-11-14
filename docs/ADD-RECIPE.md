# 添加或更新 recipe

本文件说明新增版本线、升级补丁号、更换下载地址时怎么改，以及 catalog 下载地址的判定规则。
字段含义见 [CONTRACT.md](CONTRACT.md)，版本数据规则见 `AGENTS.md` 第 4 节。

## 1. 改哪里

| 改什么 | 动哪些文件 |
| --- | --- |
| 升补丁号、换下载地址 | `recipes/catalog.toml` |
| 新增版本线 | `recipes/catalog.toml` + 消费方的 `LINES` / `PHP_LINES` / `PHP_DEP_LINES`，必要时 `recipes/bundles/php<系列>.toml` |
| 换默认线 | `recipes/bundles/php<系列>.toml` |
| 新增扩展 / 改构建参数 | `recipes/ext/<名字>.star` |
| 新增内置扩展事实 | `recipes/php<系列>.star` 的 `builtin_exts` / `builtin_disable` |
| 默认启用的扩展集合 | `recipes/bundles/php<系列>.toml` 的 `default_exts` |

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
| 不跳转 | 写原地址 | `php.net`、`pecl.php.net`、`curl.se` |

跳转后换站点的地址必须写成最终地址，原因是下载器按 catalog 里的原始主机做三件事：

- 只有 `https://github.com/` 开头才追加镜像（`crates/download/src/candidate.rs`）
- GitHub 直连用 30 秒短超时，失败后尽快换镜像
- 站点并发按主机分桶，GitHub 主站和镜像共用一个桶

写成会跳转的旧地址时，这三条规则都按旧主机判断，等于全部失效。

注释写清跳转关系，便于下次升级时核对。示例：

```toml
# OpenSSL 官方发布页只做跳转：https://www.openssl.org/source/openssl-<版本>.tar.gz
# 会 301 到 GitHub Releases，所以这里直接写跳转后的最终地址。
# 注意 1.1.1w 的 tag 是 OpenSSL_1_1_1w，3.0 及以后是 openssl-<版本>。
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

## 5. 新增版本线

除 catalog 外，还要同步：

- 有自己版本线的扩展：`recipes/ext/<名字>.star` 的 `LINES`，线集合必须和 catalog 一致
- builtin 扩展：`recipes/ext/<名字>.star` 的 `PHP_LINES`（同名依赖）或
  `PHP_DEP_LINES`（不同名依赖，如 gd 的 libpng/jpeg）
- 默认线变化：`recipes/bundles/php<系列>.toml`
- 新增 PHP 系列：`recipes/php<系列>.star` + `recipes/bundles/php<系列>.toml` + catalog `[php]`

## 6. 添加扩展

### 6.1 PHP 版本事实

新增或适配一个 PHP 版本时，在 `recipes/php<系列>.star` 写两组常量：

```python
BUILTIN_EXTS = ["openssl", "curl", "libxml", "dom", ...]

BUILTIN_DISABLE = {
    "openssl": "--without-openssl",
    "libxml": "--without-libxml",
    "dom": "--disable-dom",
    ...
}
```

规则：

- `builtin_exts` 从该 PHP 版本的 `configure --help` / 源码 `ext/` 查出来，固定成常量；不管当前有没有 recipe。
- `builtin_disable` 是每个内置扩展的关闭参数，一个扩展一个参数，允许写成字符串，不允许列表或空格。
- 只列 phprt 需要控制的内置扩展；`--enable-filter` 这类基础参数不放进表里。
- 未适配的 PHP 系列先写最小 `builtin_exts` 和 `builtin_disable`，等适配时再补全。

> 新增或修改 ext / dep recipe 前先读 [FEATURES.md](FEATURES.md)：
> 上游选项要逐个处置，并决定是否默认开启。

### 6.2 ext recipe

ext recipe 只写构建参数，不写 `origin`、`static_enable`、`static_disable`。是否内置由 `php<系列>.star` 的 `builtin_exts` 推导，通过 `ctx["is_builtin"]` 使用：

```python
def target(ctx):
    builtin = ctx["is_builtin"]
    return ext(
        name = "openssl",
        modules = ["openssl"],
        link = ["static", "shared"],
        needs = ["openssl"],
        version = ctx["php_version"] if builtin else ctx["self"]["version"],
        url = "" if builtin else ctx["self"]["url"],
        configure = ["--with-openssl={dep:openssl}"],
        shared_from = "ext/openssl",
        ini = "20-openssl.ini",
    )
```

- 内置扩展默认 static，非内置默认 shared。
- `configure` 同时用于 PHP static 和扩展自己的 shared 构建。
- 正向参数写在 ext；负向参数来自 php 版本的 `builtin_disable`。
- 内置扩展支持 shared 时必须有 `shared_from`；非内置必须有 catalog 的 url/version。
- 依赖的集成参数写 `dep_configure`，key 是 `needs` 里的依赖名，或某个 feature 的
  `needs` 里的条件依赖：

  ```python
  dep_configure = {"libcurl": ["--enable-swoole-curl"]},
  # gd 的 jpeg 由 feature 条件引入
  dep_configure = {"jpeg": []},
  ```

- 扩展自己的开关写 `features`；有条件的能力用 `requires` 声明，不要写成
  `target(ctx)` 里的 `if`：

  ```python
  features = {
      "argon2": feature(
          configure = ["--with-openssl-argon2"],
          requires = {
              "php": ["8.5", "8.6"],
              "dep": {"openssl": ["3.5", "4.0"]},
          },
      ),
  }
  ```

  显式选择不满足时报错并说明缺哪条；bundle 默认不满足时提示并丢弃。

- 需要宿主机命令的 feature 用 `requires.commands`，不要塞进 `needs`（`needs` 只认 dep recipe）：

  ```python
  features = {
      "simd": feature(
          configure = ["-DWITH_SIMD=ON", "-DREQUIRE_SIMD=ON"],
          disable = ["-DWITH_SIMD=OFF"],
          requires = {"commands": ["nasm"]},
      ),
  }
  ```

- CMake 依赖的 `configure` 第一个元素是程序：

  ```python
  configure = ["cmake", "-S", "{build}", "-B", "{build}",
               "-DCMAKE_INSTALL_PREFIX={prefix}", ...]
  make = ["cmake --build {build} --parallel", "cmake --install {build}"]
  ```

- builtin 扩展依赖多个不同名字的库时，用 `PHP_DEP_LINES`，不要写 `PHP_LINES`：

  ```python
  PHP_DEP_LINES = {
      "8.5": {"libpng": ["1.6"], "jpeg": ["3.1"], "webp": ["1.6"],
              "avif": ["1.3"], "freetype": ["2.13"]},
  }
  ```

  每个 PHP 系列都必须有入口；新增 PHP 系列时漏写会直接加载失败。

### 6.3 默认启用

默认集合写在 `recipes/bundles/php<系列>.toml` 的 `[default_exts]` 表：

```toml
[ext_presets]
gd = { features = ["jpeg", "webp", "freetype"] }

[default_exts]
openssl = { features = ["argon2"] }
curl = { deps = { libcurl = ["ssl"] } }
libxml = {}
```

规则：

- `[ext_presets]` 提供“扩展被选中时”的默认配置，不决定是否默认安装。
- `[default_exts]` 决定默认安装集合，必须放在文件最后。
- `[default_exts]` 可以用 `ref = "ext_presets.<同名扩展>"` 引用预设：

  ```toml
  [default_exts]
  gd = { ref = "ext_presets.gd" }
  ```

  使用 `ref` 时不能再写本地 `features`/`deps`；`link` 可以本地覆盖。
- 不写 `ref` 时只使用本地配置，不会隐式套同名 preset。
- key 是扩展名；每个扩展可写 `link` / `features` / `deps`，都不允许 `version:`。
- 只有 `builtin_exts` 里的扩展、并且已有 ext recipe，才能进 `default_exts`。
- 有依赖的内置扩展必须和依赖一起写，要转给依赖的 feature 显式列在 `deps` 里，例如：

  ```toml
  [default_exts]
  curl = { deps = { libcurl = ["ssl"] } }
  libxml = {}
  ```

- 不做依赖展开、不做依赖校验；漏写时 PHP configure 会报错。
- 未适配的 PHP 系列不写 `[default_exts]`，等价于空表。
- 条件 feature 在当前 PHP / 依赖线下不可用时提示并丢弃。
- 新增 PHP 系列时先写 `builtin_exts` / `builtin_disable`，再写该系列的 `[default_exts]`。

### 6.4 当前默认

只有 8.5 写了默认集：

```toml
php = "8.5"
swoole = "6.2"
xdebug = "3.5"

[deps]
openssl = "3.5"
zlib = "1.3"
libpng = "1.6"
jpeg = "3.1"
webp = "1.6"
aom = "3.12"
avif = "1.3"
freetype = "2.13"

[ext_presets]
gd = { features = ["jpeg", "webp", "freetype"] }

[default_exts]
openssl = { features = ["argon2"] }
curl = { deps = { libcurl = ["ssl"] } }
libxml = {}
```

顶层是扩展默认线，`[deps]` 是依赖库默认线；内置扩展没有版本线，不能写顶层。
`swoole` / `xdebug` / `gd` 不在默认集里，需要 `--ext` 显式选择；gd 选中时
自动使用 `[ext_presets].gd`，不需要再写 `feature:[...]`。
默认集和 preset 都只在第一次写 state 时落盘；已有项目要开 argon2 就显式传
`--ext 'openssl=feature:[argon2]'`。

## 7. 验收

```bash
cargo fmt --all
cargo test --workspace
cargo run -- install 8.5 --downloads
```

`--downloads` 会按 catalog 地址真实联网下载。网络受限时先设代理：

```bash
cargo run -- install 8.5 --downloads --proxy http://127.0.0.1:10808
```

下载完成后再跑完整安装：

```bash
cargo run -- install 8.5
```
