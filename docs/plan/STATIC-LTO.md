# 依赖静态链接与 LTO

状态：方案，未实现。实施后按第 14 节同步现行文档；文档与代码冲突时以代码实际
行为为准。

## 1. 目标

给产品加一个统一的构建开关，把依赖闭包里的**非系统库**静态链进产品本体，并在同一
次构建里启用 LTO：

```bash
phprt install nginx@1.29 --build-all-in-static=on
```

- 默认不改变现有行为：依赖仍按 shared 构建、动态链接。
- 开静态时，`openssl` / `pcre2` / `zlib` / `libxcrypt` 这类**自己声明了 static
  方式**的 dep 编成静态库，产品链接时命中 `.a`。
- 没声明 static 的库保持原样，不报错，`plan` 里写明不支持。
- glibc、libpthread、libdl、libm、ld-linux 等系统库**不**静态链接；本方案不是
  `-static` 全静态二进制。
- LTO 默认关闭；`--build-all-in-static=on` 时默认打开，也可以用 `--build-lto`
  单独控制，或在 bundle 里配置默认值。

判据（nginx）：`readelf -d sbin/nginx` 的 `NEEDED` 里不再出现
`libssl` / `libcrypto` / `libz` / `libpcre2-8`，只剩系统库。

## 2. 边界

| 对象 | 本方案的处理 |
| --- | --- |
| dep 库 | 由 recipe 声明 `shared` / `static` 两种方式；全局开关选 `static` |
| 产品本体 | 链接 `.a`，不产出静态库；nginx 还要在链接行加 LTO 参数 |
| 组件 | PHP 扩展的 `static`/`shared` 就是 builtin/shared，已有机制不动；nginx 的 `builtin`/`dynamic` 也不动 |
| dynamic-only 组件 | 例如 `lua-nginx-module` 只声明 dynamic，不参与静态，不做特殊处理 |
| 系统库 | 不静态链接 |
| PHP 的 shared 扩展 | 扩展是 `.so`，依赖静态化后无法链接，**不在本方案范围**；传了开关会在链接阶段失败 |

`static` 这个词在组件侧已经表示 builtin（fold 进产品），本方案沿用这个含义，不新增
第三种叫法。

## 3. 现状与缺口

| 机制 | 现在 | 需要补的 |
| --- | --- | --- |
| dep 构建方式 | 只有一套 `configure` / `artifacts`，没有 shared/static 声明 | `modes` + `default_mode` |
| dep 的 LTO | 没有声明；dep 节点不应用 `env`（`src/build.rs` 只对非 dep/tool 应用 profile env） | `lto` 声明 + 追加 cflags/ldflags 的通道 |
| 全局开关 | 产品 feature 只能来自 bundle，不能来自 CLI，也不进 state（`src/recipe.rs` 的 `apply_product_features`） | CLI + bundle + state 三态选项 |
| 切换后的残留 | 签名变化只跑 `make distclean`，不清 `{tree}/deps/<name>-<version>` | 重建前删 dep 安装前缀 |
| 关闭开关 | `merged_features` 只增不减，dep feature 一旦进 state 不会自动消失 | 选项作为派生状态，不做 feature 并集 |
| nginx 参数 | `--with-cc-opt` / `--with-ld-opt` 是单值 last-wins | recipe 一次拼完（见 §8） |

## 4. dep 的 modes

`dep()` 增加两个字段，都是可选的：

```python
return dep(
    name = "zlib",
    version = self["version"],
    url = self["url"],
    sha256 = self["sha256"],

    default_mode = "shared",
    modes = {
        "shared": {
            "artifacts": ["lib/libz.so"],
        },
        "static": {
            "configure": ["--static"],
            "artifacts": ["lib/libz.a"],
        },
    },
)
```

规则：

- 只认 `shared` / `static` 两个名字；`default_mode` 默认 `"shared"`，没有
  `shared` 时必须显式写。
- 没写 `modes` 的 dep 就是“不支持静态”，全局开关跳过它，保留原来的单套
  `configure` / `artifacts`。
- 选中 mode 的 `configure` 追加在基础参数之后；`artifacts` 非空时覆盖基础
  `artifacts`。
- 选中的 mode 进签名（参数和产物本来就是签名输入），切换会触发重编。
- `DepState` 增加 `mode`，`plan` / `scheme` / `state` 都能看到实际选择。

上游参数追加即可，不需要把基础参数搬进 mode：CMake 的 `-D` 和 OpenSSL 的
`shared` / `no-shared` 都是 last-wins，静态 mode 追加相反的值即可。

已知写法：

| dep | `modes.static.configure` | `modes.static.artifacts` |
| --- | --- | --- |
| zlib | `["--static"]`（也接受 `--disable-shared`） | `["lib/libz.a"]` |
| openssl | `["no-shared"]` | `["lib/libssl.a", "lib/libcrypto.a"]` |
| pcre2 | `["--disable-shared", "--enable-static"]` | `["lib/libpcre2-8.a"]` |
| libxcrypt | 已经是静态，不需要 modes | 保持 `["lib/libcrypt.a"]` |
| aom | `["-DBUILD_SHARED_LIBS=OFF"]` | `["lib/libaom.a"]` |

## 5. dep 的 lto

### 5.1 声明形状

LTO 和 shared/static 是正交的两个轴，放在 dep 顶层，不和 modes 混在一起：

```python
return dep(
    name = "aom",
    ...
    lto = {
        "configure": ["-DCMAKE_INTERPROCEDURAL_OPTIMIZATION=ON"],
    },
)
```

三个键都可选，但至少有一个：

| 键 | 用途 | 例子 |
| --- | --- | --- |
| `configure` | 上游自己的开关或 CMake 变量 | aom 的 `-DCMAKE_INTERPROCEDURAL_OPTIMIZATION=ON` |
| `cflags` | 只能走环境变量的库，追加到 `CFLAGS` | `-flto=auto -ffat-lto-objects` |
| `ldflags` | 链接期需要时追加到 `LDFLAGS` | `-flto=auto` |

- 没写 `lto` 的 dep = 不支持 LTO；全局打开时跳过，`plan` 里写 `lto unsupported`。
- 写了但三个键都为空 → recipe 校验失败。
- 应用顺序固定：
  `基础 configure → mode.configure → feature.configure → lto.configure`；
  `CFLAGS: -O2 → lto.cflags → toolchain 档位参数`；
  `LDFLAGS: -L... → lto.ldflags`。
- `cflags` / `ldflags` 是追加，不覆盖已有的 `-O2` / `-L`；两者都进签名。
- `lto` 只允许出现在 dep 上（本阶段）；组件和产品用到时再放开。

### 5.2 为什么不能统一用 configure 参数

不同库的开关不一样，而且有些库根本没有 LTO 开关：

- `aom`（CMake）：`-DCMAKE_INTERPROCEDURAL_OPTIMIZATION=ON`。CMake 自己会在编译和
  链接时加 `-flto`。
- `zlib`：只有 `--static`，configure 不认 `-flto` 之类的编译器参数；只能走
  `CFLAGS` / `LDFLAGS`。
- `openssl` 1.1.1 / 3.x / 4.0：**没有 `--enable-lto`**。它的 `Configure` 读
  `env('CFLAGS')`，但命令行上一旦出现编译 flag 就会整体忽略 env，所以 openssl 的
  `lto.configure` 必须为空，只写 `cflags` / `ldflags`。
- `pcre2` / `libxcrypt`：没有 LTO 选项，只能走 env。

所以 `configure` 是“有这个开关就用”，`cflags` / `ldflags` 是兜底通道；两者不互斥，
由 recipe 决定。

### 5.3 `-flto=auto` 与 `-ffat-lto-objects`

和 Debian 一致：

- 编译：`-flto=auto -ffat-lto-objects`；
- 链接：`-flto=auto`。

`-ffat-lto-objects` 让 `.a` 里同时保留 IR 和本机代码，宿主只有普通 `ar` / `ranlib`
（没有 `gcc-ar`）也能正常归档；代价是 `.a` 变大。

`-flto=auto` 需要 GCC 10 以上；本方案不做版本预检，编译不过就按编译器报错处理。

CMake 的 `CMAKE_INTERPROCEDURAL_OPTIMIZATION=ON` 只加 `-flto`，不带 `=auto` / fat。
如果之后要和不带 LTO 的消费者混链，再在 recipe 的 `cflags` 里补
`-ffat-lto-objects`；当前全局开 LTO 时产品自己也带 `-flto`，可以直接对上。

## 6. 全局选项

### 6.1 CLI

```text
--build-all-in-static=on|off
--build-lto=on|off
```

- 两个都是三态：不传 = 沿用 bundle，传 on/off = 显式覆盖。
- clap 用 `Option<bool>` + `num_args(0..=1)` + `default_missing_value = "on"`，
  `--build-all-in-static` 和 `--build-all-in-static=off` 都能写。
- `--build-all-in-static=on` 时 `lto` 默认跟着开；CLI 或 bundle 显式写了
  `lto=off` 以显式值为准。
- 选项在 `install` / `update` / `plan` 上生效；`add` 沿用 state，`scheme` 只输出
  state 里的有效值。

### 6.2 bundle

```toml
[build]
all_in_static = true
lto = true
```

优先级：**CLI 显式 > bundle `[build]` > 默认（都 off）**。

本阶段的全局开关只作用于 **dep**。组件继续用已有的 `modes` / 默认值：PHP 的
bundled 扩展默认就是 static/builtin，显式选了 `curl=mode:shared` 就保持 shared，
全局开关不改它。

### 6.3 state

`state.json` 增加：

```json
"build_options": { "all_in_static": true, "lto": true }
```

`deps[].mode` 记录实际选择，`deps[].lto` 记录是否启用。选项是派生状态的来源，
**不**把 `lto` 写进 `deps[].features`，避免 `merged_features` 的并集把关闭操作吞掉。

`scheme` 输出时把有效值带上：

```bash
phprt install nginx@1.29 --build-all-in-static=on --build-lto=on
```

## 7. 应用顺序

```text
读 bundle [build] + state.build_options
        ↓
CLI 覆盖
        ↓
all_in_static=on ？→ 每个 dep：有 modes.static 选 static，没有用 default_mode
lto=on ？          → 每个 dep：有 lto 声明就应用，没有跳过
        ↓
load_targets(..., options)
        ├─ ctx["build"] = { all_in_static, lto }    # recipe 可读
        └─ ctx["features"] = [本次启用的产品 feature] # nginx 拼 -L 用
        ↓
渲染 configure / env → 签名 → 构建
```

LTO 不是 dep feature，不参与 `reg_features` 的并集；它由选项直接决定。这样
`--build-lto=off` 能真正关掉，不需要清理 state 里的历史 feature。

## 8. nginx 的产品侧

nginx 的链接行是 `$(LINK) ... $(ngx_libs)`，`--with-cc-opt` 只进 CFLAGS，链接时
不会带上；所以 `-flto` 必须同时进 `--with-ld-opt`。

这两个选项是单值 last-wins，`xcrypt` 已经在用“整串重写”。再加 `lto` 会让两个 feature
互相覆盖，所以 nginx recipe 改成**一次拼完**：

```python
def link_opts(ctx):
    lto = ctx["build"]["lto"]
    cc = ["-I{dep:openssl}/include", "-I{dep:pcre2}/include", "-I{dep:zlib}/include"]
    ld = ["-L{dep:openssl}/lib", "-L{dep:pcre2}/lib", "-L{dep:zlib}/lib"]
    if "xcrypt" in ctx["features"]:
        ld.append("-L{dep:libxcrypt}/lib")
    if lto:
        cc += ["-flto=auto", "-ffat-lto-objects"]
        ld += ["-flto=auto"]
    return cc, ld
```

- 产品不使用 `lto` 字段；`ctx["build"]["lto"]` 是产品侧的入口，因为参数不是简单追加。
- `xcrypt` feature 只保留 `needs = ["libxcrypt"]`，删掉自己的 `configure`。
- `configure` 里只出现一次 `--with-cc-opt=...` 和一次 `--with-ld-opt=...`。
- 另一个候选是让 render 合并同名 `--with-*-opt`；本方案不选，避免改变其他单值选项
  的全局语义。

静态本身不需要额外的产品参数：dep 只装 `.a` 时，nginx 自己的 `-lssl` / `-lz` /
`-lpcre2-8` 就会命中静态库。`libcrypt` 同理，前提是 `xcrypt` 仍然打开（bundle 默认
打开）；关掉 `xcrypt` 就是用户选择用系统 libcrypt。

## 9. 切换与清理

切换 static/shared、或任何导致 dep 签名变化的重建，**先删**
`{tree}/deps/<name>-<version>` 再执行 configure / install。

原因：现在签名变化只跑 `make distclean`，安装前缀里的旧 `libssl.so` 不会消失，
`-lssl` 会继续优先命中 `.so`，看起来构建成功、实际还是动态链接。这和 PHP 扩展从
shared 切 static 时删旧 `.so` / ini 是同一件事；dep 目前缺这一步。

删除只针对 dep 前缀，不碰产品树和 state。切换不需要 `--force`：选项变化会让
marker 的 `inputs` 不同，走已有的增量重建路径。

## 10. 已知边界与失败模式

| 场景 | 行为 |
| --- | --- |
| dep 没有 `static` mode | 跳过，`plan` 显示 `mode=shared (static unsupported)` |
| dep 没有 `lto` 声明 | 跳过，`plan` 显示 `lto unsupported` |
| openssl 的 `lto.configure` 写了编译 flag | env CFLAGS 被忽略，LTO 静默失效；recipe 不这么写 |
| gcc < 10 | `-flto=auto` 编译失败，按编译器报错 |
| PHP shared 扩展 + 静态 dep | 缺 `.so`，链接失败；本方案不覆盖 |
| dynamic-only 组件 | 不参与静态，保持原样 |
| 不清理 dep 前缀 | 旧 `.so` 仍被命中；§9 是必需步骤 |

## 11. 落地顺序

1. **recipe 字段**：`dep()` 接受 `modes` / `default_mode` / `lto`；`src/recipe.rs`
   定义 `DepMode` / `LtoSpec` 和校验；`defs.star` 的 `dep()` 透传。
2. **构建**：`src/render.rs` 的 `dep_configure` 追加 mode / lto 参数；
   `src/build.rs` 按 mode 取 artifacts、把 `lto.cflags` / `ldflags` 追加进 dep 环境、
   重建前删 dep 前缀；`crates/build` 支持 wipe。
3. **选项**：`src/bundle.rs` 读 `[build]`；`ProjectState` 加 `build_options`；
   `src/main.rs` 加 CLI 三态、优先级和持久化；`load_targets` 注入
   `ctx["build"]` / `ctx["features"]`。
4. **nginx**：`recipes/products/nginx/1.29.star` 改成一次拼 cc/ld；
   `xcrypt` 去掉 `configure`，保留 `needs`。
5. **输出**：`plan` 显示选项、dep mode 和 lto；`scheme` 带上开关；LTO 时把编译器
   版本加入签名。
6. **文档**：README、CLI.md、BUILD.md、FEATURES.md、STATE.md、OUTPUT.md、
   CONTRACT.md、AGENTS.md。
7. **测试**：recipe 校验、优先级、签名、切换清理、nginx 实装验收。

## 12. 改动清单

| 文件 | 改动 |
| --- | --- |
| `recipes/defs.star` | `dep()` 增加 `modes` / `default_mode` / `lto` |
| `src/recipe.rs` | `DepMode` / `LtoSpec`、字段校验、`default_mode` 规则 |
| `src/render.rs` | `dep_configure` 追加 mode / lto 参数 |
| `src/build.rs` | mode artifacts、dep lto 环境、dep 前缀清理、LTO 签名 |
| `src/state.rs` | `build_options`、`deps[].mode`、`deps[].lto` |
| `src/bundle.rs` | `[build]` 表 |
| `src/main.rs` | CLI 三态、优先级、state 持久化、ctx 注入、scheme |
| `src/plan.rs` / `src/marker.rs` | 选项进入 plan 输出和 inputs |
| `crates/build/src/lib.rs` | `Step` 增加可清理前缀 |
| `recipes/dep/zlib.star` / `openssl.star` / `pcre2.star` / `libxcrypt.star` | modes / lto |
| `recipes/dep/aom.star` | modes / lto |
| `recipes/products/nginx/1.29.star` | cc/ld 一次拼完 |
| `README.md` / `docs/*.md` | 新用法和新字段 |

## 13. 验收（实施后）

```bash
# 1. 静态 + LTO
phprt install nginx@1.29 --build-all-in-static=on --offline
readelf -d .phprt/nginx-1.29.8/sbin/nginx | grep NEEDED
# 期望：没有 libssl / libcrypto / libz / libpcre2-8

# 2. dep 前缀只有静态库
ls .phprt/nginx-1.29.8/deps/*/lib
# 期望：只有 .a，没有残留 .so

# 3. LTO 对象
readelf -S .phprt/build/nginx/nginx-1.29.8/objs/src/core/nginx.o | grep -c '\.gnu\.lto_'
# 期望：大于 0

# 4. plan
phprt plan nginx@1.29 --build-all-in-static=on
# 期望：linkage static、lto on、dep mode=static / lto

# 5. 切回 shared
phprt install nginx@1.29 --build-all-in-static=off --build-lto=off --offline
readelf -d .phprt/nginx-1.29.8/sbin/nginx | grep -E 'libssl|libcrypto|libz|libpcre2'
# 期望：有动态依赖，指向树内的 lib/phprt

# 6. aom 的 LTO 参数确实下发了
grep CMAKE_INTERPROCEDURAL_OPTIMIZATION .phprt/build/logs/dep-aom-*.log
```

另需跑：

```bash
cargo test --bin phprt
cargo clippy --all-targets -- -D warnings
```

## 14. 与现有文档的关系

本文件是方案，不是已实现行为。落地时按以下关系同步：

- [FEATURES.md](../FEATURES.md) §2.1 产品 feature 一节：补充“构建选项和 feature 的
  分工”，以及 nginx 不再用 feature 拼 `--with-ld-opt`。
- [BUILD.md](../BUILD.md)：dep 的 modes、lto、前缀清理、签名。
- [CONTRACT.md](../CONTRACT.md)：`dep()` 新字段、`ctx["build"]` / `ctx["features"]`。
- [CLI.md](../CLI.md)：两个三态选项、bundle `[build]`、优先级。
- [STATE.md](../STATE.md)：`build_options`、`deps[].mode` / `lto`。
- [OUTPUT.md](../OUTPUT.md)：`plan` / `scheme` 的新字段。
- `README.md` / `AGENTS.md`：简短用法和约束。
