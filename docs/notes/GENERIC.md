# 通用化构建

状态：设计走查记录。通用化模型已经落地；与现行文档冲突时以
[DESIGN.md](../DESIGN.md)、[CONTRACT.md](../CONTRACT.md) 为准。

本文回答一个问题：怎样把 phprtv3 从“PHP 专用构建器”推广成能构建 nginx、CPython
这类产品的通用工具。设计不保留旧命令和旧 recipe 布局的兼容层：所有产品统一写成
`产品@系列`，一条命令只处理一个产品；PHP 不再是默认产品，也不引入默认版本或当前
上下文。

结论先写在前面：

- catalog / 选线 / 依赖闭包 / 下载 / 解压 / 构建执行器 / 签名跳过这些核心已经是通用的，
  真正焊死在 PHP 上的是**根产品、组件挂载协议、安装后处理**三处。
- 不需要重写求解器。需要的是把 `php` 从“唯一根类型”降成“一种产品”，把
  `ext` 从“PHP 扩展”降成“依附于产品的组件”，把 PHP 的构建/装载/验证方式收进
  产品自身的 profile。
- nginx 和 CPython 证明的是同一件事：产品自己的 configure/make 不同，组件挂到产品上的
  方式不同，但“线、依赖、拓扑、缓存、签名”都复用。

本文只给设计和迁移顺序，不修改代码。示例里的 nginx / CPython 参数是示意，落地时按
上游 `configure --help` 核对。

本文是通用化的完整走查。PHP 和 nginx 已经按此落地；CPython 仍是示例，用来验证
产品模型不依赖 autoconf/make 的固定组合。字段和命令以 [CONTRACT.md](../CONTRACT.md)、
[CLI.md](../CLI.md)、[PRODUCT.md](../PRODUCT.md) 为准；产品级工具和安装后动作以
PRODUCT.md 为准。

## 1. 现在哪些地方是 PHP 专用的

按模块列出现状。判断标准是：这里出现了 PHP 的名字、PHP 的路径、PHP 的构建命令或
PHP 的装载方式。

| 位置 | 现在的事实 | 通用化后要做的事 |
| --- | --- | --- |
| `src/catalog.rs` | `validate()` 强制存在 `[php]`；`php_series()` / `php_version()` / `find_php_series()` / `highest_php_series()` 都以 `php` 为组件名 | 产品组件由 recipe 发现；查询改成 `product_series(name)` / `product_version(name, series)`；不再强制 `[php]` |
| `src/recipe.rs` | `Kind` 只有 `Dep` / `Ext` / `Php`；根目录只允许 `php<系列>.star`；`classify()` 去掉 `php` 前缀；`Registry::php()` 要求恰好一个 PHP | 增加 `Product`；根 recipe 固定为 `products/<产品>/<系列>.star`；`Registry::product(name)`；不再识别旧的 `php<系列>.star` |
| `src/recipe.rs` | `Target.builtin_exts` / `builtin_disable`、`shared_from`、`ini`、`modules`、`origin=builtin/pecl` 都是 PHP 语义 | 产品级改成 `bundled` / `bundled_disable`；组件来源改成 `bundled` / `external`；`source_path` / `ini` 交给产品 profile 解释 |
| `src/recipe.rs` | `LineMeta.php` 是 `LINES[线]` 的必填键；`PHP_LINES` / `PHP_DEP_LINES` 按 PHP 系列建表 | `LineMeta.product`；`PRODUCT_LINES` / `PRODUCT_DEP_LINES`；不再接受 `php` 键和旧表名 |
| `src/bind.rs` | `resolve(..., php_series, ...)`；`php_in()` 过滤 `LineMeta.php`；`builtin_exts` 自动参与依赖约束 | 参数改成 `product_series`；约束按产品线求交；产品本身也进入约束集合 |
| `src/bundle.rs` | `Bundle.php` 必填；文件固定为 `bundles/php<系列>.toml`；`[ext_presets]` / `[default_exts]` | `Bundle.product`；路径改成 `bundles/<产品>/<系列>.toml`；表名改成 `[component_presets]` / `[default_components]` |
| `src/plan.rs` | 根固定是 `php`；static ext 是 php 的子节点；shared ext 在 php 之后单独展开 | 根是选中的 product；`fold` 组件是 product 的子节点；`standalone` 组件在 product 之后展开 |
| `src/build.rs` | 安装树 `.phprt/php-<版本>`；`phpize`、`--with-php-config`、`PHP_AUTOCONF`；`lib/php/extensions`；`php.ini`、`00-ssl.ini`；`php -v` / `php -m`；`myphp`、`.phprt/current` | 路径模板化；命令、验证、入口、配置文件、rpath 全交给产品 profile |
| `src/render.rs` | `Sim.tree` 固定 `.phprt/php-<版本>`；`php_configure_flags()`；shared ext 的步骤固定为 `phpize` → `configure` → `make` → `make install` | 产品名进入路径；组件步骤由 profile 的 `standalone` 模板生成；节点标签改成产品/组件 |
| `src/layout.rs` | `build_dir()` 对 `Kind::Php` 固定用 `build/php/`；下载清单只把 `pecl` 扩展算作可下载 | 产品用产品名；组件来源由 `bundled` / `external` 决定，不再判断 `pecl` |
| `src/sources.rs` | 注释和判断都是“PHP 本体、PECL 扩展” | 改成“产品本体、外部组件”；逻辑本身不用变 |
| `src/state.rs` | 状态路径 `.phprt/php-<系列>/state.json`；字段 `php`；`ext` / `deps` | 路径 `.phprt/<产品>-<系列>/`；字段 `product` / `series` / `version` / `components` / `deps` |
| `src/audit.rs` | `Kind::Ext` 才盘点；PHP 内置扩展读 `config.m4` / `config0.m4`；`reg.php()` 定位 PHP 源码 | `audit` 也覆盖产品；选项来源由 profile 声明（`configure --help`、`config.m4`、`setup.py` 等） |
| `src/main.rs` | `--php`；`install <PHP>`；`resolve_series()`；`php_id`；默认扩展必须属于 `builtin_exts`；`plan_root()` 只认 PHP | 产品统一写成 `<产品>@<系列>`；`install` 恰好一个目标；默认组件属于产品的 `bundled`；`plan_root()` 取产品节点 |

`crates/download`、`crates/extract` 不需要改；`crates/build` 的 `Cmd` / 签名 / 产物
glob / 日志执行逻辑也不需要改。它们只接收“工作目录 + 命令 + 环境 + 产物”，不关心
产品是 PHP 还是 nginx。唯一要动的是 `Step.ini`：现在它固定表示 PHP 的 ini，通用化后
改成 profile 产出的 `register` 动作（写 ini、装 `site-packages`、改配置等）。
这是通用化的基础：**执行层已经大部分通用，缺的是声明层、构建计划层和安装后登记。**

## 2. 目标模型：产品、组件、依赖

保留现有三层关系，只把名字和语义从 PHP 上解耦：

| 现名 | 通用名 | 定义 | 例子 |
| --- | --- | --- | --- |
| `php` | **product**（产品） | 一次安装的主产物；有自己的系列、补丁、源码、configure/make、验证和入口 | PHP、nginx、CPython |
| `ext` | **component**（组件） | 依附于某个产品的可选部分；可能来自产品源码包内部，也可能单独下载 | PHP 扩展、nginx 模块、CPython C 扩展 |
| `dep` | **dep**（依赖库） | 独立编译安装的库；不声明自己属于哪个产品 | OpenSSL、PCRE2、zlib、libcurl |

关系：

```text
product
├── bundled components       源码在产品 tarball 里（PHP 内置扩展、nginx 内置模块）
└── external components      独立下载（PECL 扩展、第三方 nginx 模块、PyPI 包）

product / component / dep 都可以依赖 dep
```

### 2.1 产品 recipe

产品 recipe 负责“这个产品怎么编、怎么验证、怎么放组件”。它不再把 PHP 的命令写死在
Rust 里。产品 recipe 的字段：

```python
TARGET = product(
    name = "nginx",
    series = "1.29",
    version = "",          # 由 catalog 注入
    url = "",
    sha256 = "",
    needs = ["pcre2", "openssl"],
    configure = ["./configure", "--prefix={tree}", ...],
    make = ["make", "make install"],
    artifacts = ["sbin/nginx"],
    profile = nginx_profile,
    desc = "nginx 1.29",
)
```

产品级字段：

| 字段 | 说明 |
| --- | --- |
| `name` | 产品名，如 `php` / `nginx` / `cpython` |
| `series` | 版本线，如 `8.5` / `1.29` / `3.5` |
| `version` / `url` / `sha256` | 由 catalog 注入；recipe 不写补丁号 |
| `needs` | 产品链接时需要的 dep；进入链接闭包和环境变量 |
| `deps` | 产品要转给 dep 的 feature，形如 `{ openssl = ["..."] }` |
| `bundled` | 产品源码包内自带的组件名；用于推导组件的默认挂载方式和“源码在树内” |
| `bundled_disable` | 组件未选中时产品 configure 使用的关闭参数 |
| `configure` / `make` / `artifacts` | 产品自己的构建步骤 |
| `profile` | 产品的组件挂载协议、运行时整理、验证和入口 |

现有 PHP recipe 的字段按下面的语义换成通用字段：

| 现有字段 | 通用字段 |
| --- | --- |
| `series` | `series` |
| `builtin_exts` | `bundled` |
| `builtin_disable` | `bundled_disable` |
| `configure` / `make` / `artifacts` | 同名 |
| PHP 的 `phpize` / `php-config` / `php.ini` / `php -m` | `profile` 里的 PHP profile |

### 2.2 组件 recipe

组件保留现有 `ext` 的版本、依赖、feature、prepare 和 configure 参数，但把“怎么挂到
产品上”交给产品 profile。组件只需要声明自己支持哪些挂载方式：

```python
TARGET = component(
    name = "headers-more",
    product = "nginx",
    modes = ["dynamic"],       # 产品定义的挂载方式名
    version = "",              # external 组件由 catalog 注入
    url = "",
    sha256 = "",
    needs = [],
    features = {},
    desc = "nginx headers-more 模块",
)
```

- `product`：组件属于哪个产品，必填；不再有“默认 PHP”的规则。
- `modes`：该组件支持的挂载方式。具体方式名由产品 profile 定义，不再固定为
  `static` / `shared`。
- `source = "bundled"`：源码在产品 tarball 里；`bundled` 路径由产品 recipe 或组件
  声明的相对目录决定。PHP 的内置扩展从 PHP 源码 `ext/<名字>` 取。
- `source = "external"`：组件自己带 `url` / `version`，单独下载。
- `configure` / `prepare` / `features` / `needs` / `dep_configure`：保留，含义不变。
- `artifacts`：组件的产物。`standalone` 组件必须写；`fold` 组件的产物由产品构建产生，
  可以留空，由产品 profile 的产物模式补齐。

PHP 的 `modules` / `ini` / `origin` 不再是通用字段：

- `modules` 是“产品启动后应看到的模块名”，属于产品的验证方式；
- `ini` 是“组件安装后怎么写配置”，属于产品 profile 的 `register`；
- `origin` 改成 `source`：`bundled` / `external`。

### 2.3 产品 profile：挂载协议

这是通用化最关键的一块。不同产品的组件挂载方式不同：

| 产品 | 挂载方式 | 行为 |
| --- | --- | --- |
| PHP | `static` | 组件的 configure 参数折叠进 PHP 的 `./configure`，产物在 PHP 本体里 |
| PHP | `shared` | PHP 构建完成后，用 `phpize` + `php-config` 单独编 `.so`，再写 `conf.d` ini |
| nginx | `builtin` | 组件源码路径折叠进 nginx 的 `./configure --add-module=...`，产物在 nginx 本体里 |
| nginx | `dynamic` | 同上，但用 `--add-dynamic-module=...`，产物是 nginx 的 `modules/*.so` |
| CPython | `native` | CPython 构建完成后，用产品自己的解释器编 C 扩展，装进 `site-packages` |

所以 profile 不能只写“static/shared”，而要声明**策略**和**步骤模板**。下面是 PHP 的
`products/php/_profile.star`（节选，完整内容见 [PRODUCT.md](../PRODUCT.md) §2）：

```python
PROFILE = product_profile(
    tools = [
        tool("make", min = "4.0"),
        "ar",
        "ranlib",
        tool("patchelf", min = "0.13"),
        "sh",
    ],
    modes = {
        # 折叠进产品 configure：组件不单独执行构建命令
        "static": mode(strategy = "fold"),
        # 独立构建：产品构建完后再执行组件自己的步骤
        "shared": mode(
            strategy = "standalone",
            steps = [
                "phpize",
                "./configure --with-php-config={tree}/bin/php-config {flags}",
                "make",
                "make install",
            ],
            artifacts = ["lib/php/extensions/*/{name}.so"],
        ),
    },
    # 组件来源 -> 默认挂载方式
    defaults = {"bundled": "static", "external": "shared"},
    # bundled 组件在产品源码树里的默认目录
    bundled_path = "ext/{name}",
    # 运行时整理
    runtime = runtime(
        lib_dir = "lib/phprt",
        rpath = {
            "bin/*": "$ORIGIN/../lib/phprt",
            "lib/php/extensions/*/*.so": "$ORIGIN/../../../phprt",
        },
    ),
    # 组件安装后的登记动作
    register = {
        "shared": "php-ini",
    },
    # 验证和入口
    verify = ["{tree}/bin/php", "-v"],
    verify_modules = "{tree}/bin/php -m",
    entry = "myphp-{series}",
    entry_template = "entry.sh",
    env = {
        "CC": "gcc",
        "CXX": "g++",
        "PHP_AUTOCONF": "{tool:autoconf}/bin/autoconf",
        "PHP_AUTOHEADER": "{tool:autoconf}/bin/autoheader",
    },
    post_install = [
        ensure_dir("etc/conf.d"),
        write_file("etc/php.ini", PHP_INI, if_missing = True),
        copy_ca_bundle("etc/ssl/cert.pem", [ ... ]),
        write_file("etc/conf.d/00-ssl.ini", SSL_INI),
        write_component_ini("etc/conf.d", register = "php-ini"),
    ],
    audit = "php-config-m4",
)
```

nginx 的 profile 只是换一套模式和步骤，不需要改主流程：

```python
# products/nginx/_profile.star
PROFILE = product_profile(
    modes = {
        "builtin": mode(
            strategy = "fold",
            configure = ["--add-module={component_source}"],
        ),
        "dynamic": mode(
            strategy = "fold",
            configure = ["--add-dynamic-module={component_source}"],
            artifacts = ["modules/*.so"],
        ),
    },
    defaults = {"bundled": "builtin", "external": "dynamic"},
    runtime = runtime(
        lib_dir = "lib/phprt",
        rpath = {
            "sbin/nginx": "$ORIGIN/../lib/phprt",
            "modules/*.so": "$ORIGIN/../lib/phprt",
        },
    ),
    verify = ["{tree}/sbin/nginx", "-V"],
    entry = "",
    env = {
        "CC": "gcc",
        "CXX": "g++",
    },
    audit = "none",
)
```

profile 字段的含义：

| 字段 | 说明 |
| --- | --- |
| `tools` | 产品总是需要的宿主机命令；可带最低版本，详见 [PRODUCT.md](../PRODUCT.md) |
| `post_install` | 安装后动作：`ensure_dir` / `write_file` / `copy_ca_bundle` / `write_component_ini` |
| `modes` | 产品支持的挂载方式 → `mode(strategy, steps, artifacts, configure)`；`fold` 表示组件只贡献参数/源码，`standalone` 表示产品之后单独构建 |
| `modes.*.configure` | fold 时追加到产品 `configure` 的参数模板；`{component_source}` 是组件源码根 |
| `modes.*.steps` | standalone 的步骤模板；`{flags}` 是组件的 configure 参数，`{name}` 是组件名 |
| `modes.*.artifacts` | 独立构建的产物模式 |
| `defaults` | 组件来源（bundled / external）→ 默认挂载方式 |
| `bundled_path` | bundled 组件的默认源码相对路径，支持 `{name}` |
| `register` | 组件装好后怎么让产品看见它：登记动作名，与 `post_install` 的 `write_component_ini(register=...)` 对应 |
| `runtime` | 运行时库目录和 rpath 模板 |
| `verify` / `verify_modules` | 产品启动验证；`verify_modules` 可以检查组件是否真的加载 |
| `entry` / `entry_template` | 项目入口名模板和产品目录下的模板文件（如 `entry.sh`）；PHP 的 `cli` target 用 `myphp-<系列>`，只有一个 PHP 系列时额外生成 `myphp`；有 targets 的产品把入口放在 target 里（见 [TARGETS.md](../TARGETS.md)）；nginx 不生成，CPython 可以是 `python3.5` |
| `env` | 产品专用环境变量；PHP 的 `PHP_AUTOCONF` 不再污染其他节点 |
| `audit` | 选项清单来源：`php-config-m4` / `configure-help` / `none` |

profile 只声明，不执行命令；步骤仍然由 `crates/build` 执行，签名和日志规则不变。

### 2.4 版本线与兼容性

现在的 `LINES` 是“组件线 → 支持哪些 PHP 系列、允许哪些依赖线”。通用化后把
`php` 维度改成 `product` 维度：

```python
# PHP 扩展：这条线支持哪些 PHP 产品线
LINES = {
    "6.2": {
        "product": ["8.4", "8.5"],     # 原来是 "php": [...]
        "openssl": ["1.1.1", "3.0", "3.5", "4.0"],
    },
}
```

```python
# 产品自己也可以约束依赖线：nginx 1.29 允许哪些 OpenSSL / PCRE2 线
LINES = {
    "1.29": {
        "openssl": ["3.0", "3.5", "4.0"],
        "pcre2": ["10.49"],
    },
}
```

规则：

- 组件 recipe 的 `LINES[线].product` 必填且非空；产品名必须是已加载的产品。
- 产品 recipe 的 `LINES[系列]` 可以没有 `product` 键；它的其他键是产品对依赖线的约束。
- `PHP_LINES` 推广成 `PRODUCT_LINES`：产品内置组件对同名依赖库的允许线。
- `PHP_DEP_LINES` 推广成 `PRODUCT_DEP_LINES`：产品内置组件对不同名依赖库的允许线。
- 默认线仍然只写在 bundle；CLI 显式 `version:` / `--dep` 仍然优先。
- 约束求交、硬选择报错、软默认唯一候选自动切换的规则完全不变。

### 2.5 组件来源与默认挂载方式

现在 `origin` 同时决定三件事：默认 static/shared、是否可下载、内置扩展源码从哪来。
通用化后拆成两个字段：

| 字段 | 值 | 作用 |
| --- | --- | --- |
| `source` | `bundled` / `external` | `bundled` 的源码在产品 tarball 里；`external` 有自己的 url/version |
| `modes` | 产品定义 | 组件支持哪些挂载方式；默认值由 profile 决定，不由 `source` 硬推导 |

PHP 映射：

```text
source=bundled  → 默认 static，shared 时从产品源码的 bundled 路径拷出
source=external → 默认 shared，从自己的 url 下载
```

nginx 映射：

```text
source=bundled  → 默认 builtin
source=external → 默认 dynamic
```

CPython 映射：

```text
source=external → 默认 native
```

默认值仍然必须在 `modes` 里，和现在 `link` 的契约校验一致。

## 3. 数据层怎么改

### 3.1 catalog

catalog 结构不变：仍然是“组件 + 线 → 补丁 + 下载地址”。只去掉 `[php]` 的强制要求：

```toml
[php]
"8.5" = { version = "8.5.12", url = "https://www.php.net/distributions/php-8.5.12.tar.xz", sha256 = "..." }

[nginx]
"1.29" = { version = "1.29.8", url = "https://nginx.org/download/nginx-1.29.8.tar.gz", sha256 = "..." }

[pcre2]
"10.49" = { version = "10.49", url = "https://github.com/PCRE2Project/pcre2/releases/download/pcre2-10.49/pcre2-10.49.tar.gz", sha256 = "..." }

[openssl]
"3.5" = { version = "3.5.9", url = "...", sha256 = "..." }
```

- 产品名就是 catalog 的组件名；`php` 只是其中一个产品。
- 产品线（`8.5` / `1.29` / `3.5`）和依赖线仍然只写线，不写补丁。
- 不新增 `[products]` 表：多一层会让“产品名 → 组件名”重复，现有 `[php]` 已经证明了
  直接用组件名可行。

### 3.2 bundle

bundle 从“PHP 系列的默认策略”变成“产品系列的默认策略”：

```toml
product = "nginx"
pcre2 = "10.49"

[deps]
openssl = "3.5"

[component_presets]
# nginx 模块被选中时的默认 feature；不决定是否默认安装
headers-more = { features = [] }

[default_components]
# 默认安装的组件；没有就留空
```

路径固定为：

```text
recipes/bundles/<产品>/<系列>.toml
recipes/bundles/php/8.5.toml
```

bundle 只从新路径读取；仓库里的 `php8.5.toml` 要移动到 `bundles/php/8.5.toml`，
不保留旧路径的加载分支。

字段改名：

| 现有字段 | 通用字段 |
| --- | --- |
| `php = "8.5"` | `product = "php"`，系列由文件名给出 |
| `[ext_presets]` | `[component_presets]` |
| `[default_exts]` | `[default_components]` |
| 顶层扩展默认线 | 顶层组件默认线 |
| `[deps]` | 不变 |

### 3.3 recipe 目录

布局固定为：

```text
recipes/
├── products/
│   ├── php/8.5.star
│   ├── nginx/1.29.star
│   └── cpython/3.5.star
├── components/
│   ├── php/openssl.star
│   ├── nginx/headers-more.star
│   └── cpython/demo.star
└── dep/
    ├── openssl.star
    ├── pcre2.star
    └── ...
```

仓库迁移时按下面移动文件；这是仓库本身的一次性调整，不是 loader 的兼容分支：

```text
recipes/php<系列>.star      → recipes/products/php/<系列>.star
recipes/ext/<名字>.star     → recipes/components/php/<名字>.star
```

`classify()` 的规则：

- `products/<产品>/<系列>.star` → `Kind::Product`，产品名来自目录，系列来自文件名。
- `components/<产品>/<名字>.star` → `Kind::Component`，产品名来自目录。
- `dep/<名字>.star` → 不变。
- 根目录的 `php<系列>.star` 和 `ext/` 目录不再识别；出现时报错并提示新路径。

这样新增 nginx 不需要改 `dep` 目录，也不需要把 PHP 的 recipe 留在旧位置。

### 3.4 安装树和状态

保持现有“按产品系列存方案、按补丁存安装树”的结构：

| 对象 | PHP 现在 | 通用路径 |
| --- | --- | --- |
| 状态 | `.phprt/php-8.5/state.json` | `.phprt/<产品>-<系列>/state.json` |
| 安装树 | `.phprt/php-8.5.12/` | `.phprt/<产品>-<版本>/` |
| 当前链接 | `.phprt/current → php-8.5.12` | 不再有全局 `.phprt/current`；入口名带系列，避免隐式指向某个版本 |
| 项目入口 | `myphp` | PHP 选中 `cli` target 时生成 `myphp-<系列>`；项目里只有一个 PHP 系列时再额外生成 `myphp` |
| 源码缓存 | `.phprt/downloads/` | 不变，所有产品共用 |
| 解压目录 | `build/php/<stem>` | `build/<产品>/<stem>`；dep / component 仍按名字 |
| 日志 | `build/logs/<kind>-<name>-<version>.log` | 不变 |

状态 schema 使用版本 3：

```json
{
  "version": 3,
  "active": true,
  "product": "php",
  "series": "8.5",
  "version": "8.5.12",
  "bindings": { "swoole": "6.2" },
  "targets": { "cli": {} },
  "components": {
    "swoole": {
      "line": "6.2",
      "version": "6.2.5",
      "mode": "shared",
      "installed": true,
      "deps": { "libcurl": ["ssl"] },
      "features": []
    }
  },
  "deps": {
    "openssl": { "line": "3.5", "version": "3.5.9", "features": [] }
  }
}
```

- 状态文件只接受新 schema；旧的 v2 文件（`php` / `ext` 字段）不读取，也不自动迁移。
  升级时需要删除旧的 `.phprt/*/state.json` 并按新方案重新安装，或手工转换成新格式。
- 安装树路径仍然是 `.phprt/<产品>-<版本>/`；PHP 的 `.phprt/php-8.5.12/` 正好是这套
  规则的自然结果，不是兼容别名。
- 补丁号默认跟随 catalog，`--lock` 才复用 state 记录的实际版本；`update` 只在线内换补丁。
- 多产品各自有 state 和安装树；**不共享安装树**，避免一个产品的 ABI 影响另一个产品。
  下载缓存可以共享。

## 4. 命令行

产品统一写成 `产品@系列`。一条命令只处理一个产品；没有默认产品、默认版本、当前上下文，
也没有 `phprt.toml` 或 `use`。

```bash
phprt install php@8.5
phprt install nginx@1.29
phprt install cpython@3.5

phprt plan nginx@1.29
phprt audit nginx@1.29
phprt update php@8.5
```

参数顺序固定：

```text
phprt <子命令> <产品>@<系列> [目标或组件名] [选项]
```

| 参数 | 说明 |
| --- | --- |
| `<产品>@<系列>` | 唯一目标；`install` / `update` / `plan` / `audit` / `add` / `remove` 必填，`list` 可选 |
| `--component <SPEC>` | `install` / `plan` 时临时选择组件；可重复 |
| `--dep <NAME=LINE>` | 依赖库版本线；可重复 |
| `--disable-default-component <NAME>` | 禁用产品的默认组件；可重复 |
| `--root` / `--recipes` | 工具路径选项，仍然是全局选项 |

多产品就是多条命令，不支持一条命令装多个产品：

```bash
phprt install php@8.5
phprt install nginx@1.29
phprt install mysql@8.4
phprt list
```

同一个产品的多个系列可以并存：

```bash
phprt install php@8.5
phprt install php@8.4
```

两次安装分别写 `.phprt/php-8.5/state.json` 和 `.phprt/php-8.4/state.json`，互不影响。
`list` 读取这些 state 汇总，不需要项目清单。

### 4.1 组件命令

组件概念统一叫 component；CLI 不再设 `component` 或 `ext` 子命令，直接用顶层的
`add` / `remove` / `list`：

```bash
phprt add php@8.5 swoole
phprt add php@8.5 'swoole=version:6.2,feature:[sockets,sqlite]'
phprt remove php@8.5 swoole
```

- `add` / `remove` 每次一个组件；多个组件写多条命令。
- `add` 先把组件写进该产品的 state（待安装）；真正的下载和编译由
  `phprt install php@8.5` 执行。
- 组件仍然属于某个产品；`nginx` 的模块、CPython 的 C 扩展和 PHP 扩展走同一组命令。

### 4.2 组件 spec

`add` 的组件参数和 `install --component` 使用同一种 spec：

```text
<名字>
<名字>=mode:<挂载方式>
<名字>=dep:<依赖>[feature,...]
<名字>=feature:[feature,...]
<名字>=version:<线>
上述选项的任意组合，用逗号分隔
```

```bash
phprt add php@8.5 'swoole=version:6.2,dep:libcurl[ssl]'
phprt install php@8.5 --component 'swoole=version:6.2,dep:libcurl[ssl]'
phprt install nginx@1.29 --component 'headers-more=mode:dynamic'
```

- `mode:` 取代原来的 `link:`；可选值由产品 profile 定义，例如 PHP 的 `static` /
  `shared`、nginx 的 `builtin` / `dynamic`、CPython 的 `native`。
- `dep:`、`feature:`、`version:` 的含义不变。
- 旧写法 `install 8.5`、`phprt --product ...`、`--php`、`--ext`、
  `--disable-default-ext`、`ext list`、`component add` 等直接报错，不映射到新用法。

### 4.3 命令表

| 命令 | 说明 |
| --- | --- |
| `list` | 扫描 `.phprt/*/state.json`，列出已安装的产品 |
| `list <产品>@<系列> [--installed\|--pending]` | 列出该产品的组件和状态 |
| `plan <产品>@<系列> [目标] [--json]` | 根固定是该产品；目标写 `product:<产品>@<补丁>`、`component:<名字>`、`dep:<名字>` |
| `install <产品>@<系列> [--downloads]` | 下载、解压、编译、安装；只接受一个产品 |
| `update <产品>@<系列> [--downloads]` | 在该产品线上更新补丁 |
| `add <产品>@<系列> <组件>` | 把组件加入该产品的方案（待安装） |
| `remove <产品>@<系列> <组件>` | 从该产品的方案移除组件 |
| `audit <产品>@<系列> [--json]` | 按产品 profile 的选项来源盘点 |

`install` 不接受多个产品：

```text
error: install 只接受一个产品，例如 phprt install php@8.5
```

命名冲突仍然用 `kind:name`：`dep:openssl` 是 OpenSSL 库，`component:openssl` 是 PHP 的
openssl 扩展，`product:openssl` 如果将来存在则是产品。裸名有多个候选时报歧义。
产品节点写成 `product:<产品>@<补丁>`（如 `product:nginx@1.29.8`），组件和依赖仍用
`component:<名字>` / `dep:<名字>`，这样同一个名字出现在不同层时不会混淆。

### 4.4 为什么没有 `phprt.toml`

`phprt.toml` 原本的用途是保存“当前产品/系列”，让后续命令省略 `产品@系列`。本设计放弃
默认版本和当前上下文，因此不需要它：

- 每个产品的 state 已经记录本机装了什么：`.phprt/<产品>-<系列>/state.json`；
- 多产品用多条 `phprt install`，`phprt list` 扫描 state 汇总；
- 同一个产品的多个系列可以并存，不需要一个全局“当前”指针；
- 没有清单，也就没有“清单和 state 谁优先”的冲突规则。

如果以后出现明确需求，例如团队提交“项目要装哪些产品/组件”、CI 一条命令复现，或者
多产品一次安装，再引入 `phprt.toml`。那时它的定位必须是**项目声明层**：

```text
phprt.toml                          项目期望装什么
.phprt/<产品>-<系列>/state.json      本机实际装了什么
```

不是“默认版本”，也不是“当前产品”。在这之前不引入。

## 5. 求解与构建顺序

### 5.1 求解

现有求解器只需要换维度，不需要换算法：

```text
选产品系列
  → 读 bundle（产品默认线、默认组件、组件预设）
  → 读 state.bindings / CLI
  → 组件 LINES[线].product 校验产品线
  → 产品 LINES[系列] 和组件 LINES 共同约束依赖线
  → 求交集、确定每条线
  → 注入 ctx，构造 product / component / dep 目标
  → 依赖闭包 + 拓扑
```

ctx 的通用字段：

```python
{
    "product": "nginx",
    "product_series": "1.29",
    "product_version": "1.29.8",
    "bindings": {"openssl": "3.5", "pcre2": "10.49"},
    "deps": {
        "openssl": {"line": "3.5", "version": "3.5.9", "url": "...", "sha256": "..."},
        "pcre2": {"line": "10.49", "version": "10.49", "url": "...", "sha256": "..."},
    },
    "self": {...},
    "is_bundled": True,
}
```

ctx 只提供通用字段；没有 `php_series` / `php_version` / `is_builtin` 这类按产品命名的
别名。PHP 的 recipe 通过 `product_series` / `product_version` 和 `is_bundled` 取值。

### 5.2 构建顺序

通用顺序：

```text
dep
  → fold 组件（只提供源码/参数，不执行命令）
  → product（configure / make / install）
  → standalone 组件（产品之后单独构建）
  → 运行时整理（库、rpath、配置、验证、入口）
```

映射到现有 PHP：

```text
dep → static ext → php → shared ext
```

映射到 nginx：

```text
dep:pcre2, dep:openssl
  → dynamic/builtin 模块源码
  → product:nginx（configure 带 --add-module / --add-dynamic-module）
  → 运行时整理（rpath、nginx -V）
```

映射到 CPython：

```text
dep:openssl, dep:zlib
  → product:cpython
  → native 组件（产品解释器执行扩展构建）
```

拓扑规则只需要把 `Kind::Php` 换成 `Kind::Product`，把 `Mode::Static/Shared` 换成
profile 的 `fold` / `standalone`：

- `fold` 组件是产品的子节点，只影响产品 configure 和产物；
- `standalone` 组件在 product 之后展开，依赖 product 的可执行文件；
- `dep` 仍然只通过 `needs` / feature 的 `needs` 连边；
- 成环检测、DFS 后序、工具依赖只在 `standalone` 时进 PATH，规则不变。

### 5.3 环境变量

以下变量继续通用：

| 变量 | 内容 |
| --- | --- |
| `CPPFLAGS` / `LDFLAGS` | 当前节点依赖闭包的 `-I` / `-L` |
| `PKG_CONFIG_PATH` | 依赖的 `lib/pkgconfig`、`share/pkgconfig` |
| `LD_LIBRARY_PATH` | 安装树的运行时库目录和依赖 `lib` / `lib64` |
| `PATH` | 产品 `bin`、工具 dep 的 `bin`、原 PATH |
| `MAKEFLAGS` / `CFLAGS` / `CXXFLAGS` | 不变 |

产品专用的环境变量从 profile 注入：

- PHP 的 `PHP_AUTOCONF` / `PHP_AUTOHEADER` 只在 PHP profile 里出现；
- CPython 如果需要 `PYTHONHOME` / `PYTHONPATH`，写在 CPython profile；
- nginx 不需要额外变量，直接靠 `CPPFLAGS` / `LDFLAGS` / `--with-*`。

### 5.4 安装后处理

现在 `src/build.rs` 的后处理全是 PHP：

| 现在 | 通用化 |
| --- | --- |
| `deploy_libs()` 固定收进 `{tree}/lib/phprt`，rpath 固定 `$ORIGIN` / `$ORIGIN/../lib/phprt` / `$ORIGIN/../../../phprt` | profile `runtime.lib_dir` + `runtime.rpath` 模板；默认值保持 PHP 现状 |
| `write_tree_config()` 写 `php.ini`、`00-ssl.ini`、复制 CA、写 ini | profile `post_install` 的 `write_file` / `copy_ca_bundle` / `write_component_ini`；内容在 PHP `_profile.star` |
| `verify()` 跑 `php -v` / `php -m` | profile `verify` / `verify_modules`；PHP profile 保留模块名检查 |
| `write_entry()` 写 `myphp` 和 `.phprt/current` | profile `entry`；入口名带系列，不再写全局 `.phprt/current`；默认不生成入口 |
| `log` 名字按 kind 拼 | 不变；`product` / `component` 只是新的 kind 名 |

原则：**PHP 的所有现有行为都变成 PHP profile 的默认实现，Rust 主流程不再出现 PHP 名字。**
这样 nginx 不需要 `php.ini`，CPython 不需要 ini，也不会因为共用后处理写错文件。

## 6. nginx 走查

### 6.1 catalog

```toml
[nginx]
"1.29" = { version = "1.29.8", url = "https://nginx.org/download/nginx-1.29.8.tar.gz", sha256 = "..." }

[pcre2]
"10.49" = { version = "10.49", url = "https://github.com/PCRE2Project/pcre2/releases/download/pcre2-10.49/pcre2-10.49.tar.gz", sha256 = "..." }

[openssl]
"3.5" = { version = "3.5.9", url = "...", sha256 = "..." }
```

### 6.2 依赖 recipe

`pcre2` 是普通 dep，和产品无关：

```python
def target(ctx):
    self = ctx["self"]
    return dep(
        name = "pcre2",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--enable-pcre2-8",
            "--disable-pcre2-16",
            "--disable-pcre2-32",
            "--disable-shared",
            "--enable-static",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libpcre2-8.a"],
        desc = "PCRE2，供 nginx 正则使用",
    )

TARGET = target
```

OpenSSL 复用现有 `recipes/dep/openssl.star`，线由 bundle 或 `--dep openssl=3.5` 指定。

### 6.3 产品 recipe

```python
LINES = {
    "1.29": {
        "openssl": ["3.0", "3.5", "4.0"],
        "pcre2": ["10.49"],
    },
}

def nginx_profile():
    return product_profile(
        modes = {
            "builtin": mode(strategy = "fold", configure = ["--add-module={component_source}"]),
            "dynamic": mode(
                strategy = "fold",
                configure = ["--add-dynamic-module={component_source}"],
                artifacts = ["modules/*.so"],
            ),
        },
        defaults = {"bundled": "builtin", "external": "dynamic"},
        runtime = runtime(
            lib_dir = "lib/phprt",
            rpath = {
                "sbin/nginx": "$ORIGIN/../lib/phprt",
                "modules/*.so": "$ORIGIN/../lib/phprt",
            },
        ),
        verify = ["{tree}/sbin/nginx", "-V"],
        entry = "",
        audit = "none",
    )

TARGET = product(
    name = "nginx",
    series = "1.29",
    needs = ["pcre2", "openssl"],
    configure = [
        "./configure",
        "--prefix={tree}",
        "--sbin-path={tree}/sbin/nginx",
        "--with-cc-opt=-I{dep:openssl}/include -I{dep:pcre2}/include",
        "--with-ld-opt=-L{dep:openssl}/lib -L{dep:pcre2}/lib",
        "--with-http_ssl_module",
        "--with-http_v2_module",
    ],
    make = ["make", "make install"],
    artifacts = ["sbin/nginx"],
    profile = nginx_profile(),
    desc = "nginx 1.29 本体",
)
```

要点：

- nginx 用 `--with-cc-opt` / `--with-ld-opt` 指向已安装的 PCRE2 前缀，
  让 PCRE2 只编一份（`dep:pcre2`）。动态模块和 nginx 本体共用这份共享库，
  pcre2 符号不会缺失。
- nginx 的 `--with-openssl=DIR` 期望的是 **OpenSSL 源码目录**，不是安装前缀。用它会让
  nginx 自己再编一份 OpenSSL，和 phprt 的 `dep:openssl` 重复。推荐不传
  `--with-openssl`，用 `--with-cc-opt` / `--with-ld-opt` 指向已安装的 OpenSSL；
  如果确实要 nginx 内置 OpenSSL，才用 `{source:openssl}`。
- `needs = ["pcre2", "openssl"]` 让产品的 `CPPFLAGS` / `LDFLAGS` / `LD_LIBRARY_PATH`
  带上两个库；产品的 `needs` 和 fold 组件的 `needs` 都进入链接闭包。

### 6.4 通用占位符

除了 `{tree}` / `{deps}` / `{build}` / `{prefix}` / `{dep:名字}`，nginx 模块还需要
“另一个节点的源码根”：

| 占位符 | 含义 | 例子 |
| --- | --- | --- |
| `{source:名字}` | 名字对应的 dep 或组件的解压源码根 | `{source:pcre2}`、`{source:headers-more}` |
| `{product}` | 当前产品安装树 | nginx 的 `--with-cc-opt` 可以用 |
| `{product_bin}` / 产品自定义 | 产品 profile 定义的路径 | CPython 的 `python3.5` |

`{source:名字}` 的解析顺序是 dep / component；找不到就报错，不允许猜。已实现。

### 6.5 nginx 模块

```python
TARGET = component(
    name = "headers-more",
    product = "nginx",
    source = "external",
    modes = ["dynamic"],
    version = "",
    url = "",
    sha256 = "",
    desc = "nginx headers-more 模块",
)
```

- 组件只提供源码和模式，不写 `configure`；profile 的 `fold.dynamic` 负责生成
  `--add-dynamic-module={component_source}`。
- 组件是 external，所以会被下载、解压，排在 product 之前（fold 组件不执行命令）。
- 产物是 nginx 的 `modules/*.so`，由产品 profile 的 `artifacts` 检查，不由组件节点检查。
- 如果某个模块需要自己的 configure/make，就把模式改成 `standalone`，在 profile 里给
  对应模式写步骤模板；不需要为单个模块在 Rust 里加分支。

## 7. CPython 走查

CPython 用来验证“产品构建系统不是 autoconf/make 的固定组合”也能接入。

### 7.1 catalog 和产品

```toml
[cpython]
"3.5" = { version = "3.5.10", url = "https://www.python.org/ftp/python/3.5.10/Python-3.5.10.tgz", sha256 = "..." }
```

```python
LINES = {
    "3.5": {
        "openssl": ["1.0.2", "1.1.1"],
        "zlib": ["1.3"],
    },
}

CPython = product_profile(
    modes = {
        "native": mode(
            strategy = "standalone",
            steps = [
                "{product}/bin/python3.5 -m pip install --no-build-isolation --prefix={tree} {build}",
            ],
            artifacts = ["lib/python3.5/site-packages/{name}*"],
        ),
    },
    runtime = runtime(
        lib_dir = "lib",
        rpath = {
            "bin/python3.5": "$ORIGIN/../lib",
            "lib/python3.5/lib-dynload/*.so": "$ORIGIN/../../..",
        },
    ),
    verify = ["{tree}/bin/python3.5", "-V"],
    entry = "python3.5",
    audit = "setup-py",
)

TARGET = product(
    name = "cpython",
    series = "3.5",
    needs = ["openssl", "zlib"],
    configure = [
        "./configure",
        "--prefix={tree}",
        "--enable-shared",
        "--with-ensurepip=install",
    ],
    make = ["make", "make install"],
    artifacts = ["bin/python3.5"],
    profile = CPython,
    desc = "CPython 3.5",
)
```

### 7.2 C 扩展组件

```python
TARGET = component(
    name = "demo",
    product = "cpython",
    source = "external",
    modes = ["native"],
    version = "",
    url = "",
    sha256 = "",
    desc = "示例 C 扩展",
)
```

- 产品先构建，组件依赖产品的 `bin/python3.5`；拓扑序自动满足。
- 组件不需要 `phpize` / `ini` / `source_path`，这些字段不会出现在 CPython 的 profile 里。
- `artifacts` 用 `site-packages` 模式检查；具体路径按 CPython 版本核对。

CPython 3.5 在现代工具链和 OpenSSL 3 上未必能直接编过；这里只验证产品模型，
不推导 CPython 的版本兼容性。

## 8. 实现顺序

这次是破坏性变更：最终版本不保留旧命令、旧 recipe 路径和旧 state。

当前落地状态：第 1～5 步、第 7 步已经完成（PHP 和 nginx 真构建通过）；第 6 步
CPython 仍未实现，只保留为模型示例。

### 第 1 步：产品模型

- 增加 `Kind::Product`，删除 `Kind::Php`；`php` 变成 `Product(php, ...)`。
- `Registry::php()` 改成 `Registry::product(name)`；主流程只依赖本次命令选中的产品。
- catalog / bundle / state 的路径和字段按产品模板化。
- `LineMeta.php` 改成 `LineMeta.product`，不留 `php` 键。

### 第 2 步：PHP profile

- 把 `src/build.rs` 里的 PHP 后处理、`src/render.rs` 里的 shared 组件步骤、`audit.rs`
  的 `config.m4` 逻辑收进 PHP profile。
- profile 只描述，不执行；主流程按 profile 生成 `Step`。
- 目标：PHP 的构建树、rpath、ini 和验证结果按现有逻辑复现；选中 `cli` target 时
  入口按新规则生成 `myphp-<系列>`，只有单个 PHP 系列时额外生成 `myphp`。

### 第 3 步：契约改名

- `php()` → `product()`；`ext()` → `component()`；`builtin_exts` → `bundled`；
  `source_path` 取代 `shared_from`；`origin` → `source`；`is_builtin` → `is_bundled`。
- `LINES[].php` → `LINES[].product`；`PHP_LINES` → `PRODUCT_LINES`；
  `PHP_DEP_LINES` → `PRODUCT_DEP_LINES`。
- 一次改完 recipe 和 loader，不留旧 key 的读取分支。

### 第 4 步：补产品需要的通用能力

- 产品 `needs` / `deps` 进入链接闭包；
- `{source:名字}` 占位符；
- 产品级 `LINES` 约束依赖线；
- profile 的 `tools` / `modes` / `defaults` / `bundled_path` / `runtime` /
  `register` / `post_install` / `verify` / `entry` / `env` / `audit`。

### 第 5 步：nginx

- 新增 `recipes/products/nginx/1.29.star`、`recipes/dep/pcre2.star`；
- 用已有 OpenSSL dep；
- 先做一个 `dynamic` 模块，验证 fold 模式；
- 验收 `nginx -V`、`ldd`、模块加载。

### 第 6 步：CPython

- 新增 `recipes/products/cpython/3.5.star`；
- 用一个 `native` 组件验证 standalone 模式；
- 验收 `python3.5 -V` 和扩展 import。

### 第 7 步：CLI 和状态切换

- 产品统一写成 `<产品>@<系列>`；`install` / `update` / `plan` / `audit` 一次只接受一个产品；
- `add` / `remove` 用顶层命令，不保留 `component` / `ext` 子命令；
- 不引入 `phprt.toml`、`use` 或默认版本；多产品用多条 `install`，`list` 扫描 state 汇总；
- `--php`、`--product`、`--series`、`--ext`、`--disable-default-ext`、`ext *`、
  `component *`、`install 8.5` 直接报错；
- state 只读写 v3；v2 不读取，不自动迁移。

## 9. 风险与未决

1. **产品 profile 可能变成隐藏的 DSL。** 只允许声明步骤模板、路径和验证命令，不在
   profile 里写条件分支；复杂的“按版本不同”仍然放 recipe 的 `target(ctx)`。
2. **nginx 的 OpenSSL 是源码还是安装前缀要定死。** 推荐链接已安装的 `dep:openssl`，
   避免 nginx 自己再编一份；这要求产品 `needs` 真的进入 `CPPFLAGS` / `LDFLAGS`。
3. **组件挂载方式的名字不再固定。** 旧的 `--ext link:static|shared` 是 PHP 词汇；
   新 CLI 用 `--component mode:<方式>`，方式名由产品定义（`static/shared`、
   `builtin/dynamic`、`native`）。旧写法直接报错，不做转换。
4. **多产品共享依赖库不做。** 第一版每个产品一棵安装树，下载缓存共享；跨产品共享
   同一份 OpenSSL 会引入 ABI 和 rpath 耦合，留到以后。
5. **`audit` 的选项来源不是所有产品都有。** 没有 `configure --help` / `config.m4`
   的产品把 profile 的 `audit` 记 `none`，该产品的组件不要求盘点选项；PHP 用
   `php-config-m4`，继续按现有规则要求处置或 `ignored`。
6. **CPython 3.5 只当模型示例。** 不推导它对现代编译器、OpenSSL 3 的兼容性。
7. **名称冲突。** `product:openssl` / `component:openssl` / `dep:openssl` 可以同时存在，
   裸名必须报歧义；这沿用现有 `kind:name` 规则。
8. **`make distclean` 是执行器的假设。** 签名变化时 `crates/build` 会先跑
   `make distclean` 并忽略失败；CMake 源码内构建没有 distclean，执行器随后会删
   `CMakeCache.txt` / `CMakeFiles`。其他构建系统如有残留对象，需要把清理命令写进
   对应的构建步骤。

## 10. 验收

通用化的验收分两层：先证明 PHP 没退化，再证明第二个产品能编。

```bash
# 1. PHP
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets
cargo run -- install php@8.5 --downloads
cargo run -- plan php@8.5 --json
cargo run -- add php@8.5 swoole
cargo run -- list php@8.5

# 2. nginx
cargo run -- install nginx@1.29 --dep openssl=3.5
./.phprt/nginx-1.29.8/sbin/nginx -V
ldd ./.phprt/nginx-1.29.8/sbin/nginx | grep -E 'ssl|crypto|pcre2'
ls ./.phprt/nginx-1.29.8/modules/ 2>/dev/null || true

# 3. nginx 组件
cargo run -- add nginx@1.29 lua-nginx-module
cargo run -- install nginx@1.29
ls .phprt/nginx-1.29.8/modules/

# 4. 路径和状态
test -f .phprt/php-8.5/state.json
test -f .phprt/nginx-1.29/state.json
test -x myphp-8.5
test -x myphp                 # 只有一个 PHP 系列时的别名
test ! -L .phprt/current      # 不再写全局 current
cargo run -- list
cargo run -- install php@8.5 nginx@1.29   # 必须失败：一次只能一个产品
find .phprt/downloads -maxdepth 1 -type f   # 不应有直接放在根目录的源码包
```

判定标准：

- PHP 的 `plan` 节点、线、依赖顺序、configure 参数（含选中 target 的 flags）、产物
  （含选中 target 的 artifacts）、ini 和验证结果与设计前一致；选中 `cli` target 时
  入口是 `myphp-8.5`，只有一个 PHP 系列时才额外生成 `myphp`；只选 `fpm` / `cgi`
  时不生成入口。
- nginx 的 `plan` 里 `dep:pcre2` / `dep:openssl` 在 `product:nginx` 之前；nginx 的
  configure 能看到 OpenSSL / PCRE2 的 include/lib；构建产物 `sbin/nginx` 存在。
- nginx 的动态模块 `modules/ngx_http_lua_module.so` 存在，`nginx -V` 能看到
  `--add-dynamic-module`。
- `--downloads` 跨产品共用 `.phprt/downloads`，不重复下载同一个 URL。
- 失败仍然不写 state；签名跳过、日志末尾、产物检查规则不变。

## 11. 不在本文范围

- lock 文件、精确补丁固定；
- 一次命令构建多个产品；
- 跨产品共享安装树；
- 交叉编译、LTO、`--static-all`；
- CPython 3.5 / nginx 具体版本的兼容矩阵推导。

这些都可以在“产品 / 组件 / 依赖”三层之上追加，不需要再改一次核心模型。
