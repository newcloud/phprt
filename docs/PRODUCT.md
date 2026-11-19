# 产品 recipe：profile、工具与安装后动作

本文说明怎样把产品相关的行为从 Rust 里搬进 recipe：

- 一个产品一份共享 profile，同产品的所有系列共用；
- 产品 profile 声明构建工具、工具最低版本、安装后动作和入口模板；
- 系列 recipe 只写版本事实和差异（更高的工具要求、特殊 CFLAGS）；
- Rust 只保留机制：探测版本、读写文件、复制 CA、写 ini、执行动作。

字段总表在 [CONTRACT.md](CONTRACT.md)，命令在 [CLI.md](CLI.md)，构建执行细节在
[BUILD.md](BUILD.md)。

## 1. 目录

产品级文件固定在 `recipes/products/<产品>/`：

```text
recipes/products/php/
├── _profile.star     # 所有 PHP 系列共用的 profile；不是系列 recipe
├── entry.sh          # 项目入口模板；不是 .star，不会被当成 recipe
├── 7.4.star
├── 8.0.star
└── 8.5.star
```

- `_profile.star` 以 `_` 开头，loader 不把它当作系列 recipe，而是在求值同目录的
  系列文件之前先求值它。
- 系列 recipe 通过 `profile = PROFILE` 使用共享 profile。
- `entry.sh` 是普通文件，只被 `entry_template` 引用。

## 2. 共享 profile

```python
# recipes/products/php/_profile.star

PHP_INI = """[PHP]
date.timezone = Asia/Shanghai
memory_limit = 256M
error_reporting = E_ALL
display_errors = On
log_errors = On
"""

SSL_INI = """; 由 phprt 生成
openssl.cafile = "${SSL_CERT_FILE}"
curl.cainfo    = "${SSL_CERT_FILE}"
"""

PROFILE = product_profile(
    tools = [
        tool("make", min = "4.0"),
        "ar",
        "ranlib",
        tool("pkg-config", min = "0.29"),
        tool("patchelf", min = "0.13"),
        tool("perl", min = "5.20"),
        tool("m4", min = "1.4"),
        "sh",
    ],
    modes = {
        "static": mode(strategy = "fold"),
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
    defaults = {"bundled": "static", "external": "shared"},
    bundled_path = "ext/{name}",
    runtime = runtime(
        lib_dir = "lib/phprt",
        rpath = {
            "bin/*": "$ORIGIN/../lib/phprt",
            "lib/php/extensions/*/*.so": "$ORIGIN/../../../phprt",
        },
    ),
    register = {"shared": "php-ini"},
    backup = backup(
        relative_dir = "backup",
        name = "{timestamp}_{component}.ini",
    ),
    post_install = [
        ensure_dir("etc/conf.d"),
        write_file("etc/php.ini", PHP_INI, if_missing = True),
        copy_ca_bundle("etc/ssl/cert.pem", [
            "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/pki/tls/certs/ca-bundle.crt",
            "/etc/ssl/cert.pem",
        ]),
        write_file("etc/conf.d/00-ssl.ini", SSL_INI),
        write_component_ini("etc/conf.d", register = "php-ini"),
    ],
    verify = ["{tree}/bin/php", "-v"],
    verify_modules = "{tree}/bin/php -m",
    entry = "myphp-{series}",
    entry_template = "entry.sh",
    env = {
        "CC": "gcc",
        "CXX": "g++",
    },
    audit = "php-config-m4",
)
```

系列 recipe 只写版本事实：

```python
# recipes/products/php/8.5.star
TARGET = product(
    name = "php",
    series = "8.5",
    bundled = BUILTIN_EXTS,
    bundled_disable = BUILTIN_DISABLE,
    configure = [...],
    make = ["make", "make install"],
    artifacts = ["bin/php"],
    profile = PROFILE,
)
```

## 3. 系列差异：`extend_profile` 与 `toolchain`

需要按系列改工具要求或环境变量时，用 `extend_profile(PROFILE, ...)`。
编译器版本和兼容参数不走 `extend_profile`，写在系列自己的 `TOOLCHAIN` 里：

```python
# recipes/products/php/8.0.star
TOOLCHAIN = toolchain(
    gcc = [
        toolchain_range(min = "9", max = "13"),
        toolchain_range(
            min = "14",
            cflags = "-std=gnu17 -Wno-error=incompatible-pointer-types",
        ),
    ],
    gxx = [toolchain_range(min = "9")],
)

TARGET = product(
    name = "php",
    series = "8.0",
    ...
    profile = PROFILE,
    toolchain = TOOLCHAIN,
)
```

需要提高工具要求时在同一个调用里加 `tools`，例如
`tools = [tool("make", min = "4.3")]`；同名的会替换 profile 里的那一条。

合并规则固定：

| 字段 | 规则 |
| --- | --- |
| `tools` | 按名字合并；同名时后面的替换前面的；可以新增 |
| `env` | 按 key 合并；后面的覆盖前面的 |
| `modes` / `defaults` / `runtime` / `register` / `post_install` / `verify` / `verify_modules` / `entry` / `entry_template` / `audit` / `bundled_path` | 传了才替换，没传沿用 base |

系列里的 `env` 会覆盖 profile 的 `env`，所以可以在共享 `CC` / `CXX` 的基础上
只加该系列需要的变量。`runtime.lib_dir` 同时作为构建时 `LD_LIBRARY_PATH` 的首项
和运行时收库目录。

`env` 里的 `{tool:NAME}` 只在当前构建节点的 `tools` 含该工具、且这个工具解析到
built 版本时展开成 `build/tools/<name>-<version>`；宿主提供的工具跳过这个 env 项，
让宿主 PATH 自己找到命令（见 [TOOLS.md](TOOLS.md) §3.4）。phpize 的
`autoconf` / `autoheader` 直接从 PATH 找，不再用 `PHP_AUTOCONF` / `PHP_AUTOHEADER`。

最低版本只用于“明显编不了”的检查；编译器太新带来的行为变化（例如 GCC 15 默认
`gnu23`）必须由 recipe 显式处理，不能根据版本号自动推导；这类参数写在 §4.3 的
toolchain 档位里。

## 4. 工具声明

```python
tool(name, min = "", max = "", probe = ["--version"])
```

- 字符串缩写表示只检查存在：`"patchelf"`。
- `min` / `max` 是版本区间，数字点分：`"11"`、`"4.2"`。
- 按写出的段数比较：`min = "14"` 接受 14.0.1，`max = "13"` 接受 13.9.9、
  拒绝 14.0；空表示不限。
- `probe` 是探测命令的参数列表，不经过 shell；默认 `--version`。
- 版本从 stdout + stderr 里取第一个 `\d+(\.\d+)+` 片段。
- 探测时设置 `LC_ALL=C`。

示例：

```python
tool("cmake", min = "3.20", probe = ["--version"])
tool("nasm", min = "2.13", probe = ["-v"])
tool("perl", min = "5.20", probe = ["-e", "print $^V"])
```

解析发生在下载源码之前，按节点进行：宿主满足区间就用宿主，否则复用
`build/tools` 里已有的版本，再否则从 catalog 取满足区间的最高版本源码构建。
宿主有命令但版本不满足时不会静默回退，错误会写清是哪一种情况：

```text
宿主 cmake 3.10 不满足 >= 3.20，catalog 也没有可用版本
宿主缺少 nasm，catalog 没有 nasm 的构建 recipe
catalog 里 autoconf 最高 2.72，不满足 >= 9.0
```

宿主工具不下载、不安装；built 工具装到 `build/tools/<name>-<version>`，跨产品复用。
`--downloads` 只下载本次解析到、需要构建的工具源码，不检查、不构建。

### 4.1 检查的命令要和实际使用的一致

PHP / nginx 的 configure 默认调用 `cc`。如果只检查 `gcc`，系统 `cc` 可能指向别的
编译器。产品 profile 用 `env` 固定：

```python
env = {"CC": "gcc", "CXX": "g++"}
```

这样检查和构建使用同一个编译器。

### 4.2 条件工具

产品级 `tools` 放“这个产品总是需要”的命令。只在某些依赖 / feature 下才需要的命令
写在对应 recipe：

```python
dep(
    name = "aom",
    tools = [tool("nasm", min = "2.13"), "cmake"],
    ...
)
```

```python
dep(
    name = "libzip",
    tools = ["cmake"],
    ...
)
```

`host_tools` 已删除，构建期工具统一写在 `tools`。解析按节点进行：产品 profile 的
`tools` 是产品 / 依赖 / 组件节点的基线，再并上该节点自己的 `tools`；同名取交集
（`min` 取大、`max` 取小），交集为空时报错。

同名工具允许在同一次构建里解析出多个版本：不同节点可以落到不同版本，
`build/tools/<name>-<version>` 并存。工具节点只用自己的 `tools`，不继承产品
profile 的基线（否则工具会被绑回某个产品的依赖树，无法跨产品共享）。

被选中 feature 的 `requires.commands` 由选择阶段检查，不进入这个集合。

## 4.3 编译器版本与 toolchain

gcc / g++ 不写在 `tools` 里，也不从 Tool catalog 解析；产品在系列 recipe 里声明
编译器区间和兼容参数档位。完整机制见 [TOOLS.md](TOOLS.md) 第 4 节。

产品在系列 recipe 里声明编译器区间和兼容参数档位：

```python
TOOLCHAIN = toolchain(
    gcc = [
        toolchain_range(min = "4.8", max = "13"),
        toolchain_range(
            min = "14",
            cflags = "-std=gnu17 -Wno-error=incompatible-pointer-types",
        ),
    ],
    gxx = [
        toolchain_range(min = "4.8", max = "13"),
        toolchain_range(min = "14"),
    ],
)

TARGET = product(
    ...,
    toolchain = TOOLCHAIN,
)
```

- 只声明 `gcc` / `gxx`；**不声明 glibc / ld / binutils**。它们跟随 gcc 所在的
  系统或镜像，只影响产物能否在目标系统运行。
- 宿主构建：探测实际编译器（`env.CC` / `CXX` 优先，其次进程环境，缺省
  gcc / g++），不满足区间就报错；不下载、不安装、不回退。
- 同一项里的档位不能重叠；只探测声明了的项，没声明 `gxx` 就不检查 g++。
- 选中档的 `cflags` / `cxxflags` 追加到节点的 `CFLAGS` / `CXXFLAGS`，顺序是
  `宿主 CFLAGS -> recipe/env -> 档位`，保证档位参数最后生效。档位参数作用于产品、
  依赖和组件节点，不作用于 Tool 节点。
- **gcc / g++ 的原始版本号不进节点签名**：同一档内换补丁版本不重编，跨档改变
  flags 才重编。Tool 节点另算：`tool-sig` 记实际编译器版本。
- 镜像构建：用镜像 catalog 里的 `gcc` 匹配同一套区间；`glibc` 只提示运行下限，
  不参与构建判断。详见 [IMAGE.md](IMAGE.md)。
- 编译器太新带来的 flags 只写在档位里，Rust 不按版本号推导。
- dep / component 默认不写 gcc；确实有已知不兼容时写一条只收窄的约束，
  只求交、不带 flags。

7.4 / 8.0 原先写在 `env.CFLAGS` 里的 `-std=gnu17 -Wno-error=...` 已迁到
`>= 14` 档，和上面一样按 `gcc` / `gxx` 两项声明。

## 4.4 构建目标（targets）

产品可以在系列 recipe 的 `product(targets = ...)` 里声明构建目标，例如 PHP 的
`cli` / `fpm` / `cgi`。声明写在各系列自己的文件里，不写进 `_profile.star`；版本差异
直接在该系列覆盖。

产品声明了 targets 时，以下字段由选中的 target 提供，profile 里的同名字段只作为
没有 targets 时的普通路径：

| 内容 | 来源 |
| --- | --- |
| artifacts | 产品 `artifacts` + 选中 target 的 `artifacts` |
| post_install | profile 的 `post_install` + 选中 target 的 `post_install` |
| verify / verify_modules | 每个选中 target 的 `verify` / `verify_modules` |
| entry / entry_template | 选中 target 里声明了 `entry` 的那一个 |

多个选中 target 同时声明 `entry` 时构建报错。完整规则见 [TARGETS.md](TARGETS.md)。

## 5. 安装后动作

`post_install` 是动作列表，只允许以下四类：

| 动作 | 参数 | 行为 |
| --- | --- | --- |
| `ensure_dir(path)` | 相对安装树 | 创建目录 |
| `write_file(path, content, if_missing = False)` | 相对安装树 | 写文件；`if_missing` 时保留已有文件 |
| `copy_ca_bundle(to, candidates)` | 相对安装树 | 找第一个存在的候选并复制；都不存在时警告，不失败 |
| `write_component_ini(dir, register)` | 相对安装树 | 为选中且匹配 `register` 的组件写 ini，删掉旧 ini |

规则：

- 路径必须相对安装树，拒绝绝对路径、`..` 和空路径。
- `write_file` 的 `content` 按原样写入，不做占位符展开，避免破坏 `php.ini` 里的
  `${SSL_CERT_FILE}` 等 shell 变量。
- `copy_ca_bundle` 目标已存在时跳过；目标目录不存在时需要先用 `ensure_dir` 建好
  （PHP 的 `post_install` 在复制 CA 之前 `ensure_dir("etc/ssl")`）。
- `write_component_ini` 使用组件 recipe 的 `ini` / `ini_lines`；没有 `ini` 的组件忽略。
- 动作没有任意 shell 逃生口；需要改上游源码的命令仍然写在组件的 `prepare` 里。

执行顺序：

```text
runtime（收库、rpath）
  → post_install（目录、文件、CA、ini）
  → verify
  → entry
```

`post_install` 每次安装都执行，不做签名跳过；它是幂等的文件操作，开销很小。

### 5.1 登记文件备份

`write_component_ini` 会重写或删除组件 ini；产品可以在 profile 里声明备份规则：

```python
backup = backup(
    relative_dir = "backup",                 # 相对工具目录（默认 .phprt，安装树所在）
    name = "{timestamp}_{component}.ini",     # 必须含 {component}
)
```

- 只会备份 `write_component_ini` 管理的登记文件，不备份 `.so` 和安装版本；
- 文件不存在、或内容和将要写入的一致时不备份；
- 文件名模板支持 `{timestamp}`（`YYYYMMDD-HHMMSS`）、`{component}`、`{mode}`；
- 重名依次加 `-2`、`-3`；
- 不写 `backup` 的产品（如 nginx）不做任何备份；
- 用户可以用 `--no-backup-php-ext-ini` 关掉本次备份（`--force` 不隐含此效果）；
- 备份位置由 `relative_dir` 相对工具目录给出，也可以用 `phprt.toml` 的 `local_backup_dir`
  或 `--backup-dir` 整体覆盖（可绝对或相对）。

## 6. 入口模板

没有 targets 的产品由 profile 声明；有 targets 的产品把
`entry` / `entry_template` 写在选中的 target 里（见 [TARGETS.md](TARGETS.md)）。

产品 profile 声明：

```python
entry = "myphp-{series}"
entry_template = "entry.sh"
```

Rust 读取 `recipes/products/<产品>/<entry_template>` 的内容，替换：

| 标记 | 展开为 |
| --- | --- |
| `@@TREE@@` | `<产品>-<产品版本>`，相对 `.phprt/` |
| `@@TARGET@@` | `<产品>@<系列>`，用在错误提示里 |

然后把文件写到项目根并加可执行位。入口模板路径必须是同一目录下的普通文件名，
不能包含 `/` 或 `..`。

PHP 只有一个 PHP 系列时，额外写一个去掉 `-<系列>` 的别名入口；nginx 不生成入口。

## 7. Rust 与 recipe 的边界

| 放在 recipe | 留在 Rust |
| --- | --- |
| 需要哪些工具、最低版本、探测参数 | 探测命令、解析版本、比较版本 |
| 安装后写哪些文件、内容、CA 候选 | 创建目录、写文件、复制 CA、错误处理 |
| 哪些组件挂载方式要写 ini | 按选择结果写 / 删 ini |
| 产品入口名和模板文件 | 读模板、替换标记、写可执行文件 |
| 系列的特殊 CFLAGS / 工具要求 | 把 env 注入构建环境 |
| 挂载方式、rpath、验证命令 | 执行步骤、patchelf、日志、签名 |

Rust 中不再出现 `php.ini`、`00-ssl.ini`、CA 候选列表、`myphp` 模板、`gcc` /
`patchelf` / `nasm` 固定名单。

## 8. nginx 示例

```python
# recipes/products/nginx/_profile.star
PROFILE = product_profile(
    tools = [
        tool("make", min = "4.0"),
        "ar",
        "ranlib",
        tool("perl", min = "5.20"),
        tool("patchelf", min = "0.13"),
        "sh",
    ],
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
    audit = "none",
)
```

系列 recipe 只写 `profile = PROFILE` 和版本事实。

## 9. 不在范围

- 按单个组件 / feature 自动推导工具版本；
- 根据工具版本自动生成 CFLAGS；
- 在 `post_install` 里执行任意 shell；
- 为每个系列维护一份完整 profile（差异用 `extend_profile`）。

## 10. 验收

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets

cargo run -- install php@8.5
./myphp-8.5 -v
test -f .phprt/php-8.5.12/etc/ssl/cert.pem
cargo run -- audit php@8.5

cargo run -- install nginx@1.29
./.phprt/nginx-1.29.8/sbin/nginx -V
```
