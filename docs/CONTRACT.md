# recipe 契约

recipe 是 `recipes/**/*.star`。三层目标：产品（product）、组件（component）、
依赖库（dep）。`defs.star` 提供 `dep` / `component` / `product` /
`product_profile` / `mode` / `runtime` / `feature` 构造函数；校验全部在 Rust 侧。

目录固定为：

```text
recipes/
├── products/<产品>/<系列>.star
├── components/<产品>/<名字>.star
├── dep/<名字>.star
├── bundles/<产品>/<系列>.toml
├── catalog.toml
└── defs.star
```

- `products/<产品>/<系列>.star` 的 `TARGET` 必须是 `product(...)` dict，不能是函数；
  补丁、下载地址由 catalog 注入。
- `components/<产品>/<名字>.star` 的 `TARGET` 可以是 dict 或 `target(ctx)` 函数。
- 根目录的 `php<系列>.star` 和 `ext/` 目录不再识别，出现时报错并提示新路径。

一个组件 / 依赖 recipe 可以写成两种形式：

```python
# 静态 dict
TARGET = dep(...)

# 依赖注入：Rust 先解析版本线，再带着 ctx 调用
def target(ctx):
    ...
    return component(...)

TARGET = target
```

`ctx` 由 Rust 注入：

```python
{
    "product": "php",
    "product_series": "8.5",
    "product_version": "8.5.12",
    "bindings": {"openssl": "3.5", "swoole": "6.2"},
    "deps": {
        "openssl": {"line": "3.5", "version": "3.5.9", "url": "...", "sha256": "..."},
        "libcurl": {"line": "8.23", "version": "8.23.0", "url": "...", "sha256": "..."},
    },
    "self": {"line": "6.2", "version": "6.2.5", "url": "...", "sha256": "..."},
    "is_bundled": False,
}
```

- `self` 是当前 recipe 同名组件在 catalog 里的记录；bundled 组件没有记录，为 `None`。
- `is_bundled` 由产品 recipe 的 `bundled` 推导；dep / product 恒为 `False`。
- 版本号、下载地址、sha256 都从 ctx 取，recipe 不写死。
- bundled 组件用 `ctx["product_version"]` 作为自己的版本，url/sha256 留空；
  external 组件用 `self` 的版本和下载地址。

## 1. 版本关系

产品与依赖库版本的对应关系属于**产品 recipe 和组件 recipe**，不属于 dep。

### 组件自己的版本线：`LINES`

有独立版本线的组件（如 swoole、lua-nginx-module）声明：

```python
LINES = {
    "6.2": {
        "product": ["8.4", "8.5", "8.6"],           # 这条线支持哪些产品系列
        "openssl": ["1.1.1", "3.0", "3.5", "4.0"],  # 依赖允许的线（可选）
    },
}
```

- `product` 必填且非空。
- 除了 `product`，其他 key 都当作依赖约束：依赖名 → 允许的版本线。
- 如果声明了 `LINES`，线集合必须和 `catalog` 里同名组件的线完全一致。

### 产品自己的版本线：`LINES`

产品 recipe 也可以用 `LINES` 约束依赖线，不需要 `product` 键：

```python
# products/nginx/1.29.star
LINES = {
    "1.29": {
        "openssl": ["3.0", "3.5", "4.0"],
        "pcre2": ["10.49"],
    },
}
```

### bundled 组件的依赖线：`PRODUCT_LINES`

`component:openssl` 没有自己的版本线（版本跟 PHP 走），用 `PRODUCT_LINES` 声明
“这个 PHP 的 openssl 组件允许哪些 OpenSSL 库线”：

```python
PRODUCT_LINES = {
    "7.4": ["1.1.1"],
    "8.5": ["1.1.1", "3.0", "3.5", "4.0"],
}
```

- key 必须是该产品的 catalog 系列。
- value 里的线必须存在于 catalog 的同名组件（`component:openssl` → `openssl`）。
- 只在选中该组件时参与约束。

### 没有同名依赖的 bundled 组件：`PRODUCT_DEP_LINES`

gd 这类内置组件依赖多个不同名字的库，不能写 `PRODUCT_LINES`。用
`PRODUCT_DEP_LINES` 按产品系列列出每个依赖允许的线：

```python
# components/php/gd.star
PRODUCT_DEP_LINES = {
    "8.5": {
        "zlib": ["1.3"],
        "libpng": ["1.6"],
        "jpeg": ["3.1"],
        "webp": ["1.6"],
        "aom": ["3.12"],
        "avif": ["1.3"],
        "freetype": ["2.13"],
    },
}
```

- 内层 key 必须是 `dep/*.star` 里的依赖名；允许的线必须存在于 catalog。
- 只在选中该组件时参与约束，和 `PRODUCT_LINES` 一样进入依赖线交集。

### dep 不写产品关系

`dep` recipe 禁止声明 `LINES` / `PRODUCT_LINES` / `PRODUCT_DEP_LINES`。OpenSSL、
libcurl、nghttp2 与 PHP 没有直接关系；哪个 PHP 能用哪条 OpenSSL 线，由消费它的组件
或产品决定。

`target(ctx)` 只负责构建参数，不判断能不能配。

## 2. catalog

`recipes/catalog.toml` 存“组件 + 线 → 当前补丁 + 下载地址”，需要时再登记旧补丁：

```toml
[php]
"8.4" = { version = "8.4.14", url = "https://www.php.net/distributions/php-8.4.14.tar.gz",
          patches = [
              { version = "8.4.13", url = "https://www.php.net/distributions/php-8.4.13.tar.gz" },
              { version = "8.4.12", url = "https://www.php.net/distributions/php-8.4.12.tar.gz" },
          ] }
"8.5" = { version = "8.5.12", url = "https://www.php.net/distributions/php-8.5.12.tar.xz" }

[nginx]
"1.29" = { version = "1.29.8", url = "https://nginx.org/download/nginx-1.29.8.tar.gz" }

[swoole]
"6.2" = { version = "6.2.5", url = "https://pecl.php.net/get/swoole-6.2.5.tgz", sha256 = "..." }

[openssl]
"3.5" = { version = "3.5.9", url = "https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz", sha256 = "..." }
```

- 产品名就是 catalog 的组件名；不再强制 `[php]`。
- 每条线必须有 `version` 和 `url`；`version` 是这条线的当前补丁。
- `patches` 可选，登记同一条线的旧补丁（`version` / `url` / `sha256`），
  让 `install php@8.4.13` 能装指定补丁；不写就只能装当前补丁。
- `patches` 里的补丁不能和当前补丁重复，版本号不能重复。
- `url` 写上游最终地址；跳转链路用注释记录，判定规则见 [ADD-RECIPE.md](ADD-RECIPE.md)。
- 不写产品兼容性、不写构建参数。
- 补丁更新只改这里。

## 3. bundle

`recipes/bundles/<产品>/<系列>.toml` 写默认线和默认组件：

```toml
product = "php"
swoole = "6.2"
xdebug = "3.5"

[deps]
openssl = "3.5"

# 组件被选中时的默认 feature；不决定是否默认安装
[component_presets]
openssl = { features = ["argon2"] }
curl = { deps = { libcurl = ["ssl"] } }
gd = { features = ["jpeg", "webp", "freetype"] }

# 默认启用的组件；必须放在文件最后
[default_components]
openssl = { ref = "component_presets.openssl" }
curl = { ref = "component_presets.curl" }
libxml = {}
```

- `product` 必须和目录名一致；系列由文件名给出。
- 顶层键只写**有独立版本线的组件**（如 `swoole`、`xdebug`）；值只能是 catalog 里存在的线。
  bundled 组件（如 `openssl`、`curl`、`libxml`）没有自己的版本线，不能写在顶层。
- `[deps]` 只写依赖库，key 必须是 `dep/*.star` 里的目标；值是该库在 catalog 里的线。
  bundled 组件的同名库版本写在这里，例如 `openssl = "3.5"` 指 OpenSSL 库，不是 PHP 的
  openssl 组件。
- 同一个名字不能同时出现在顶层和 `[deps]`。
- `[component_presets]` 和 `[default_components]` 都是表，key 是组件名；
  `[default_components]` 必须放在文件最后。
- `[component_presets]` 只在组件被 `--component` / `add` 选中时提供默认
  `mode` / `features` / `deps`，**不会默认安装组件**。
- `[default_components]` 决定默认安装集合。每个组件支持：
  - `ref = "component_presets.<名字>"`：引用预设；后缀必须和组件名一致。
  - `mode = "static" | "shared" | ...`：不写时按 source 推导；使用 ref 时仍可写 mode 覆盖 preset。
  - `features = ["名字", ...]`：组件自己的编译开关。
  - `deps = { 依赖名 = ["feature", ...] }`：转给依赖的 feature。
- 使用 `ref` 时不能再写本地 `features` / `deps`；需要不同配置时另建 preset。
- 不写 `ref` 时只使用本地配置，不会隐式套同名 preset。
- 不允许写 `version:`；`[default_components]` / `[component_presets]` 里的名字必须是当前
  产品系列的 component recipe。
- 有依赖的 bundled 组件必须在 preset 或 `default_components` 里和依赖一起写，要转给依赖的
  feature 显式列在 `deps` 里（如 `[component_presets] curl = { deps = { libcurl = ["ssl"] } }`）；
  不做依赖展开或校验，漏写时产品 configure 会报错。
- 默认 feature 在当前产品 / 依赖线下不可用时，提示并丢弃，不中断命令。
- 选中组件时的 feature 优先级：CLI 显式 `feature:[...]`（含 `[]`） > state >
  `[component_presets]` > 空。preset 只在组件还没有 state 时生效。

## 4. dep

```python
def target(ctx):
    self = ctx["self"]
    return dep(
        name="openssl",
        version=self["version"],
        url=self["url"],
        sha256=self["sha256"],
        needs=[],
        features={
            "ssl": feature(
                needs=["openssl"],
                configure=["--with-openssl={dep:openssl}"],
                disable=["--without-ssl"],
            ),
        },
        configure=["./Configure", "--prefix={prefix}", "shared"],
        make=["make", "make install_sw"],
        artifacts=["lib/libssl.so.3"],
        desc="OpenSSL 库",
    )

TARGET = target
```

字段：

- `version` / `url` / `sha256` 由 ctx 注入。
- 安装前缀是 `.phprt/<产品>-<产品版本>/deps/<name>-<version>`。
- 禁止产品 / 组件字段。
- `tools` 是这个依赖构建时需要的工具，如 `["cmake"]` 或
  `[tool("nasm", min = "2.13")]`；宿主满足就用宿主，否则从 catalog 构建到
  `build/tools/`。`host_tools` 已删除。
- `features` 由组件的 `dep:NAME[feature]` 间接启用；没启用的 feature 会应用它的
  `disable` 参数（用来关掉上游的自动探测），没有 `disable` 就不传。
- dep 的 feature 允许写 `requires.commands`（宿主机命令）；不允许 `requires.product` /
  `requires.dep`。例如 libjpeg 的 SIMD：

  ```python
  features = {
      "simd": feature(
          configure = ["-DWITH_SIMD=ON", "-DREQUIRE_SIMD=ON"],
          disable = ["-DWITH_SIMD=OFF"],
          requires = {"commands": ["nasm"]},
      ),
  }
  ```

- dep 的 `configure` 第一个元素是程序，其余是参数；CMake 依赖写
  `configure=["cmake", "-S", "{build}", ...]`，`make` 用
  `cmake --build {build}` / `cmake --install {build}`。不要在 recipe 里写 `--parallel`：
  并发数由 `phprt.toml` 的 `parallel` / `--parallel` 通过 `CMAKE_BUILD_PARALLEL_LEVEL` 给。
- `ignored` 是「看过但决定不暴露」的上游选项 -> 理由；`phprt audit` 会对照上游清单检查。

## 5. component

```python
def target(ctx):
    self = ctx["self"]
    return component(
        name="swoole",
        product="php",
        source="external",          # bundled / external
        modes=["shared"],           # 契约允许的挂载方式
        version=self["version"],
        url=self["url"],
        sha256=self["sha256"],
        modules=["swoole"],
        needs=["libcurl", "openssl"],
        tools=[tool("autoconf", min = "2.68")],
        dep_configure={"libcurl": ["--enable-swoole-curl"]},
        configure=["--enable-swoole", "--with-openssl-dir={dep:openssl}"],
        ini="27-swoole.ini",
        desc="Swoole 扩展，契约只支持动态编译",
    )

TARGET = target
```

规则：

- `product` 必填；是否 bundled 由产品 recipe 的 `bundled` 推导。
- `source` 只能是 `bundled` / `external`：
  - `bundled` 的源码在产品源码树里；`source_path` 覆盖产品 profile 的 `bundled_path`
    （PHP 默认是 `ext/{name}`）。
  - `external` 必须有自己的 `url` + `version`。
- `modes` 非空，值由产品 profile 定义；默认挂载方式由 profile 的 `defaults` 按 source 推导。
- `modules` 是产品启动后应看到的模块名，由产品 profile 的 `verify_modules` 使用。
- `ini` / `ini_lines` 是安装后的登记文件，只有 profile 的 `register` 动作为 `php-ini` 时使用。
- `tools` 是构建期工具要求：宿主满足就用宿主，否则从 `recipes/tools/` 构建到
  `build/tools/<name>-<version>`；参与构建顺序和 `PATH`，不参与 include/lib，
  也不进入产品运行时库收集。只有 standalone 组件解析自己的 `tools`。
- `host_tools` 已删除，构建期工具统一写在 `tools`。
- `prepare` 是在 standalone 构建之前于源码目录执行的 shell 命令列表，用来给上游源码
  打兼容补丁（如删除已移除头文件的 include），也可以做显式的源码文件准备，例如把只有
  `config0.m4` 的 PHP 扩展准备成 `config.m4`。命令写进构建签名，变化会触发重编。
  只有 component 能用，dep / product 报错；通用执行器不会按文件名自动处理。
- `needs` 是链接和运行时需要的依赖库；产品运行时只收集它的闭包和已启用 feature 的
  `needs`，不会收集 `tools` 指向的构建工具。
- `env` 是组件自己的环境变量；fold 时合并进产品节点，standalone 时用于自己的构建节点。
- `configure` 是正向参数：fold 时交给产品的 configure，standalone 时交给组件自己的
  （或 profile 步骤里的）configure。
- 关闭参数不在组件 recipe 里，来自产品 recipe 的 `bundled_disable`。
  同一个 bundled 组件只会出现 `--with-*` 或 `--without-*` 其中一组。
- `dep_configure` 是“依赖名 → 选中 `dep:NAME` 时额外加给组件的 configure 参数”：
  - key 必须是 `needs` 里的依赖，或某个 feature 的 `needs` 里的条件依赖；
  - 值为空列表表示“只允许转发 feature，不加参数”。

  gd 的 jpeg 是条件依赖，所以可以写：

  ```python
  needs = ["libpng"]
  dep_configure = {"jpeg": []}
  features = {"jpeg": feature(needs=["jpeg"], configure=["--with-jpeg"])}
  ```

- `features` 是组件自己的开关：

  ```python
  features = {
      "argon2": feature(
          configure = ["--with-openssl-argon2"],
          requires = {
              "product": ["8.4", "8.5", "8.6"],
              "dep": {"openssl": ["3.5", "4.0"]},
          },
      ),
  }
  ```

  - 允许 `needs` / `configure` / `requires`；`disable` 只用于 dep 的 feature。
  - `needs` 是**选中这个 feature 时**才加入构建顺序和链接闭包的依赖
    （例：swoole 的 `cares` 需要 `c-ares`，不选就不编 c-ares）。
  - `requires.product` 是允许的产品系列；`requires.dep` 是允许的依赖线，key 必须在
    `needs` 里；`requires.commands` 是需要出现在 PATH 里的宿主机命令（如 `nasm`）。
  - 显式选择不满足时报错；bundle 默认不满足时提示并丢弃。
- `ignored` 记录看过但不暴露的上游选项，值为理由（不能为空）；`audit` 会对照上游清单
  检查遗漏和拼写错误。取舍规则见 [FEATURES.md](FEATURES.md)。
- 版本线通过 `LINES` / `PRODUCT_LINES` / `PRODUCT_DEP_LINES` 声明，不在 recipe 里写死。

## 5.1 tool

```python
def target(ctx):
    self = ctx["self"]
    return tool_target(
        name="autoconf",
        version=self["version"],
        url=self["url"],
        sha256=self["sha256"],
        tools=[tool("m4", min="1.4")],
        configure=["./configure", "--prefix={prefix}"],
        make=["make", "make install"],
        artifacts=["bin/autoconf"],
        desc="构建期工具：phpize 需要 autoconf",
    )

TARGET = target
```

规则：

- 路径是 `recipes/tools/<名字>.star`，节点 id 是 `tool:<名字>@<版本>`。
- 版本、url、sha256 由 catalog 注入；catalog 里每条线（含 `patches`）都是一个可选版本。
- 安装前缀是 `build/tools/<名字>-<版本>`，源码目录仍是 `build/<名字>/<stem>`。
- 不能声明 `needs`（工具不能依赖库），只能声明 `tools`（工具依赖工具）。
- 不允许产品 / 组件字段：`product` / `series` / `source` / `modes` / `bundled` /
  `components` / `targets` / `prepare` / `features` / `ignored`。
- 工具按 `--prefix={prefix}` 安装；`artifacts` 相对安装前缀检查。
- 工具节点不继承产品 profile 的 `tools`，也不带产品的 toolchain 兼容参数。

## 6. product 和 profile

```python
TARGET = product(
    name="php",
    series="8.5",
    bundled=["openssl", "curl", "libxml", ...],
    bundled_disable={
        "openssl": "--without-openssl",
        "libxml": "--without-libxml",
        "dom": "--disable-dom",
    },
    configure=["--prefix={tree}", "--with-config-file-scan-dir={tree}/etc/conf.d"],
    make=["make", "make install"],
    artifacts=[],
    profile=PROFILE,
    targets=TARGETS,
    desc="PHP 8.5 本体",
)
```

`targets` 是产品构建目标声明，target 名 → 声明：

```python
TARGETS = {
    "cli": {
        "flags": ["--enable-cli"],
        "disable_flags": ["--disable-cli"],
        "artifacts": ["bin/php"],
        "config": {},
    },
    "fpm": {
        "flags": ["--enable-fpm"],
        "disable_flags": [],
        "artifacts": ["sbin/php-fpm"],
        "config": {
            "user": {"type": "string", "flags": "--with-fpm-user={value}"},
        },
    },
}
```

- target 名和配置项名只允许字母、数字、`-`、`_`。
- `type` 只支持 `string` / `list`；`flags` 必须包含 `{value}`。
- 每个 target 可选 `artifacts` / `post_install` / `verify` / `verify_modules` /
  `entry` / `entry_template`；产品声明了 targets 时由选中 target 提供，见
  [TARGETS.md](TARGETS.md)。
- 默认选择只在 bundle 的 `[targets] default`，recipe 不写默认 target。
- target 声明写在各系列自己的文件里，不写进 `_profile.star`；版本差异直接在文件里覆盖。

- 路径是 `products/<产品>/<系列>.star`；`series` 必须和文件名一致。
- 同一产品的所有系列共用 `products/<产品>/_profile.star` 里的 `PROFILE`；
  系列差异用 `extend_profile(PROFILE, ...)` 覆盖，详见 [PRODUCT.md](PRODUCT.md)。
- `version` / `url` / `sha256` 从 catalog 的同名产品注入。
- `needs` / `deps`：产品链接和运行时需要的依赖库，以及要转给它们的 feature。
- `bundled`：该产品源码包内自带的组件名，和是否有 recipe 无关；不要求非空。
- `bundled_disable`：bundled 组件名 → 单个关闭参数；key 必须在 `bundled` 里。
- `toolchain` 是 `toolchain(gcc=[...], gxx=[...])`，声明编译器版本档位和每档的
  兼容参数；不写就不检查、不加参数。只有 `gcc` / `gxx` 两项，档位不能重叠，
  `gcc` 档位只写 `cflags`，`gxx` 档位只写 `cxxflags`。详见 [TOOLS.md](TOOLS.md) §4。
- `profile` 是 `product_profile(...)`，同产品的系列共用
  `products/<产品>/_profile.star` 里的 `PROFILE`：

  ```python
  PROFILE = product_profile(
      tools = [tool("make", min = "4.0"), tool("patchelf", min = "0.13"), "sh"],
      modes = {
          "static": mode(strategy="fold"),
          "shared": mode(
              strategy="standalone",
              steps = ["phpize", "./configure --with-php-config={tree}/bin/php-config {flags}",
                       "make", "make install"],
              artifacts = ["lib/php/extensions/*/{name}.so"],
          ),
      },
      defaults = {"bundled": "static", "external": "shared"},
      register = {"shared": "php-ini"},
      backup = backup(relative_dir = "backup", name = "{timestamp}_{component}.ini"),
      post_install = [
          ensure_dir("etc/conf.d"),
          write_component_ini("etc/conf.d", register = "php-ini"),
      ],
      verify = ["{tree}/bin/php", "-v"],
      entry = "myphp-{series}",
      entry_template = "entry.sh",
  )
  ```

  完整字段和 PHP 的实际 profile 见 [PRODUCT.md](PRODUCT.md) §2；系列的
  `extend_profile` 覆盖规则见 §3。字段含义：

| 字段 | 说明 |
| --- | --- |
| `tools` | 产品总是需要的构建期工具；`tool(name, min, max, probe)` 或字符串，宿主优先、否则从 catalog 构建，详见 [PRODUCT.md](PRODUCT.md) 与 [TOOLS.md](TOOLS.md) |
| `modes` | 挂载方式 → `mode(strategy, steps, artifacts, configure)` |
| `defaults` | 组件来源（bundled / external）→ 默认挂载方式 |
| `bundled_path` | bundled 组件的默认源码相对路径，支持 `{name}` |
| `runtime.lib_dir` | 依赖库统一运行目录，相对安装树；同时作为构建时 `LD_LIBRARY_PATH` 的首项；空表示不整理 |
| `runtime.rpath` | 安装树内相对 glob → rpath |
| `register` | 挂载方式 → 登记动作名；要和 `post_install` 里 `write_component_ini` 的 `register` 一致 |
| `backup` | 组件登记文件的备份规则 `backup(relative_dir, name)`；`relative_dir` 相对工具目录（默认 `.phprt`，构建后的安装树所在），`name` 支持 `{timestamp}` / `{component}` / `{mode}` 且必须含 `{component}`；不写就不备份 |
| `post_install` | 安装后动作列表：`ensure_dir` / `write_file` / `copy_ca_bundle` / `write_component_ini` |
| `verify` / `verify_modules` | 产品启动验证；`verify_modules` 用来检查组件模块 |
| `entry` / `entry_template` | 项目入口名模板和产品目录下的模板文件名（如 `entry.sh`） |
| `env` | 产品专用环境变量，支持 `{tree}`、`{tool:<dep>}` |
| `audit` | 选项来源：`php-config-m4` / `configure-help` / `none` |

`mode(strategy=...)`：

- `fold`：组件只贡献 configure 参数和源码，编进产品本体；`configure` 里的模板追加到
  产品 configure，支持 `{component_source}` / `{name}`。
- `standalone`：产品构建完之后，按 `steps` 单独构建；支持 `{name}` / `{product}` /
  `{flags}` / `{build}` / `{prefix}`；`artifacts` 支持 `{name}`。

### 6.1 动作契约

通用执行器只实现机制，产品差异全部由 recipe 声明；Rust 核心不判断产品名。
各动作的职责、顺序和签名规则：

| 动作 | 声明位置 | 职责 | 执行顺序 | 变量解析 | 签名规则 |
| --- | --- | --- | --- | --- | --- |
| `env`（profile） | `product_profile(env=...)` | 产品专用环境变量，如 PHP 的 `CC` / `CXX` | 构建节点环境的一部分，先于命令执行 | `{tree}` / `{tool:NAME}`；只有节点 `tools` 含该工具且解析到 built 版本时展开，宿主提供时跳过该项 | 展开后的值进签名 |
| `env`（component） | `component(env=...)` | 组件专用环境变量 | 在 profile env 之后合并 | `{tree}` / `{dep:}` / `{source:}` 等占位符 | 展开后的值进签名 |
| `prepare` | `component(prepare=[...])` | 源码准备，如把 `config0.m4` 准备成 `config.m4`、打兼容补丁 | standalone 构建之前、源码拷贝之后、profile steps 之前 | 同 component env | 命令原文进签名 |
| `configure` | `dep` / `component` / `product` 的 `configure` | 生成构建文件 | prepare 之后、make 之前 | 占位符展开；dep 追加 feature 参数 | 展开后的全部参数进签名 |
| `steps`（profile mode） | `mode(steps=[...])` | standalone 组件的构建步骤模板 | prepare 之后，按声明顺序 | `{name}` / `{product}` / `{flags}` / `{build}` / `{prefix}` 先替换，再展开占位符 | 替换后的命令进签名 |
| `make` | 各 target 的 `make` | 编译 / 安装 | configure 之后 | 占位符展开 | 展开后的命令进签名 |
| `runtime` | `runtime(lib_dir, rpath)` | 运行时收库和 rpath | 全部构建完成之后 | 不适用 | `lib_dir` / `rpath` 变化会改变环境或 patchelf 输入，间接进签名 |
| `post_install` | `product_profile(post_install=[...])` | 安装后整理：目录、文件、CA、组件 ini | 构建完成、收库之后，按声明顺序 | 固定原语，不执行任意 shell | 动作参数进 recipe 内容，间接进签名 |
| `verify` / `verify_modules` | `product_profile(...)` | 安装后验证 | `post_install` 之后 | `{tree}` | 不进签名（只验证不重编） |
| `entry` / `entry_template` | `product_profile(...)` | 项目入口 | 安装完成时 | `{series}` | 不进签名 |
| `targets`（product） | `product(targets=...)` | 构建目标声明：flags / disable_flags / config / 可选产物与安装动作 | 产品 configure 和安装阶段 | target 名、配置名规范化为排序结构 | configure 参数和产物进签名；targets 文本进方案指纹 |
| `targets`（bundle） | `[targets] default` | 该系列默认选哪些 target | 没有 CLI / state 选择时 | 只写 target 名 | 不直接进签名 |

边界：执行器只负责「按顺序展开占位符并执行」；选什么参数、什么步骤、什么环境，
由 PHP / nginx 等产品 recipe 自己声明。新增产品行为时先查这张表，
能在现有动作里表达就不要加 Rust 分支。

## 7. 占位符

| 占位符 | 展开为 |
| --- | --- |
| `{tree}` / `{product}` | `.phprt/<产品>-<产品版本>` |
| `{deps}` | `{tree}/deps` |
| `{build}` | 当前节点的解包/构建目录 |
| `{prefix}` | 当前节点的安装前缀 |
| `{dep:名字}` | `{tree}/deps/<名字>-<版本>` |
| `{source:名字}` | 对应 dep / component 的解压源码根 |
| `{component_source}` | 仅 fold 模板：组件源码根 |
| `{tool:名字}` | built 工具的安装前缀 `build/tools/<名字>-<版本>`；宿主提供时不生成该 env 项 |

未知占位符在渲染阶段报错。`{source:名字}` 先找 dep，再找 component；找不到直接报错。

## 8. 校验清单

加载时检查：

1. `name` 非空；dep 的 version/url；product 的 series/version/url。
2. dep 不出现产品 / 组件字段。
3. component 的 `product`、`source`、`modes`、默认 mode；external 必须有 version/url；
   component 不允许产品字段。
4. `needs` 指向的 `dep:*` 存在。
5. `tools` 每项名字非空；工具在解析阶段按 catalog 选择版本，不要求 registry 里已有同名目标。
6. `features.*.needs` 指向的 `dep:*` 存在。
7. `dep_configure` 的 key 在 `needs` 或某个 feature 的 `needs` 里；component 的
   `features.*.requires.dep` 的 key 也在 `needs` 里。`requires.commands` 每项非空；
   dep 的 feature 只允许 `commands` 条件。`ignored` 只能出现在 dep / component 上，
   key 是 `--...` 选项，理由非空。
8. product 的 `toolchain` 至少要声明 `gcc` / `gxx` 之一；每个档位必须有 min 或 max，
   同一项里的档位不能重叠，`gcc` 档位不能写 `cxxflags`、`gxx` 档位不能写 `cflags`。
9. product 的 `targets` 名字、配置项名、类型、`{value}` 模板必须合法；声明了 targets
   时，cli / state / bundle 里的未知 target 或未知配置项都直接报错，不静默丢弃。
10. component 的 `modes` 必须在产品 profile 的 `modes` 里；默认 mode 也必须在 `modes` 里。
11. product 的 `bundled` 无重复；`bundled_disable` 的 key 是其子集，值是单个
   `--without-*` / `--disable-*`；profile 的 `modes` 非空、`strategy` 合法。
12. catalog 的产品系列与 product recipe 集合一致。
13. catalog 里每个组件（含 tool）都有同名 recipe；组件声明 `LINES` 时线集合必须一致。
14. `LINES` 的每条线对组件必须有非空 `product` 数组；产品可以省略 `product`。
15. dep / tool recipe 不声明 `LINES` / `PRODUCT_LINES` / `PRODUCT_DEP_LINES`。
16. `PRODUCT_LINES` / `PRODUCT_DEP_LINES` 只用于 component，引用的系列和线都在 catalog 里。
17. 既不是当前产品 bundled、也没有 catalog 记录的 component recipe 不加载。
18. `[component_presets]` 自己不能写 `ref`；`[default_components]` 的 `ref` 必须是
    `"component_presets.<同名组件>"`，指向存在的 preset，且不能和本地 `features`/`deps`
    同时写。
