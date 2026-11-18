# 构建工具与工具链（Tool / Toolchain）

**本文是待实施的开发设计，代码尚未实现。** 本文只定机制、字段和验收；实施时
另起提交，并按第 7 节同步现有文档。与代码冲突时以代码实际行为为准。

目标是把现在混在 `host_tools`、`profile.tools`、`Target.tools` 和普通 dep 里的
构建期工具拆清楚：

- **Tool**：patchelf、cmake、nasm、autoconf、bison、re2c 这类独立构建辅助程序。
  优先用宿主机；宿主不满足时从 catalog 取源码构建，装到 `build/tools/`。
- **Toolchain**：gcc / g++ / glibc / binutils 这类基础编译环境。产品声明版本档位，
  每档可以带兼容 CFLAGS；Rust 只探测、选档、记录，不下载 gcc/glibc，也不启动容器。

两者都不新建一套平行的构建管线：Tool 复用现有 dep 节点的下载、解压、构建、日志、
签名和跳过逻辑；Toolchain 只做环境探测和参数合并。

---

## 1. 现状与问题

先纠正几个容易误解的事实：

| 现状 | 说明 |
| --- | --- |
| 没有 Tool Catalog | `recipes/catalog.toml` 里只有 `[autoconf]` 一条构建期工具的源码记录；patchelf、cmake、nasm 都只是宿主机命令检查 |
| autoconf 已经能源码构建 | 它走的是 dep 管线：catalog `[autoconf]` + `recipes/dep/autoconf.star` + 组件的 `tools = ["autoconf"]` + `PHP_AUTOCONF = "{tool:autoconf}/bin/autoconf"` |
| `host_tools` 只检查 | `src/tools.rs` 只探测 PATH 和版本；实际版本不进签名 |
| `profile.tools` 完全不进签名 | `signature()` 只遍历节点的 `host_tools`，产品 profile 的 gcc / patchelf / m4 等不参与任何节点签名 |
| 工具的安装前缀在产品树里 | autoconf 装到 `.phprt/<产品>-<版本>/deps/autoconf-2.72`，产品重建会换掉整棵树 |

由此产生四个问题：

1. **语义不对**：安装 PHP 时输出 `[dep] autoconf`，但 autoconf 不是 PHP 的库依赖。
2. **宿主机有工具也照编**：`tools = ["autoconf"]` 总是把 autoconf 当 dep 构建，没有
   host-first。
3. **工具跟产品树同生命周期**：产品重建、换安装树会丢掉工具；工具本应跨产品、
   跨系列复用。
4. **签名看不见实际工具**：系统 cmake / autoconf 升级不会触发任何重编；反过来，
   改了工具 recipe 也不会让使用它的节点重编。

Toolchain 方面，现状是：

- gcc / g++ / make / perl / m4 / pkg-config / patchelf 都写在产品 profile 的 `tools`
  里，只检查最低版本；
- 编译器太新带来的兼容参数写在系列 recipe 的 `env.CFLAGS` 里，**无条件应用**；
- 没有 glibc / ld / target triple 的记录，也没有按编译器版本分档的机制。

当前 `recipes/products/php/7.4.star` 和 `8.0.star` 已经有：

```python
env = {
    "CFLAGS": "-O2 -std=gnu17 -Wno-error=incompatible-pointer-types",
}
```

这说明 PHP 7.4 在 GCC 15 上不是“不能编”，而是需要 GCC 14+ 的兼容参数。本文把
这类参数收进 Toolchain 的版本档位，而不是用 `max` 把 GCC 15 拒掉。

---

## 2. 概念边界

| 概念 | 典型成员 | 归谁 | 是否编译 | 是否部署 |
| --- | --- | --- | --- | --- |
| 库依赖（dep） | openssl、libxml2、libzip | `recipes/dep/` | 是 | 随产品收库 |
| 组件（component） | PHP 扩展、nginx 模块 | `recipes/components/` | 是 | 是 |
| **Tool** | autoconf、cmake、nasm、patchelf | `recipes/tools/` | 宿主不满足时编译 | 否 |
| **Toolchain** | gcc、g++、glibc、ld | 产品 `toolchain(...)` | 不编译 | 否 |

判定规则：

- 只要它**会被构建命令执行**、且不是链接进产物的库，就是 Tool。
- 只要它决定**编译器和基础运行库**，就是 Toolchain。
- 不要只凭名字判断：`cmake` 是 Tool，`libcmake` 若是库则仍是 dep；`autoconf` 是
  Tool，`m4` 也是 Tool，但 `libxml2` 不是。
- Toolchain 不下沉到 dep / component。产品声明的 gcc 范围覆盖整次构建；依赖库在
  这个范围里编不过，是 bundle / recipe 的问题，不是给单个库再声明一套 toolchain。

---

## 3. Tool

### 3.1 目录与节点

```text
recipes/
├── products/
├── components/
├── dep/
└── tools/
    └── autoconf.star
```

Recipe 形态：

```python
TARGET = tool_target(
    name = "autoconf",
    configure = ["./configure", "--prefix={prefix}"],
    make = ["make", "make install"],
    artifacts = ["bin/autoconf"],
    tools = [tool("m4", min = "1.4")],   # 工具依赖工具，可选
    desc = "构建期工具：phpize 需要 autoconf",
)
```

内部加 `Kind::Tool`，节点 id 为 `tool:<name>`。它复用现有 dep 节点的：

- catalog 版本选择、下载、sha256 校验、解压；
- `configure` / `make` / `artifacts` 执行；
- 构建日志、`.phprt-sig` 跳过、增量复用；
- `--downloads`、`--offline`、`--lock` 的源码缓存。

不新建 `ToolStore`、`ToolResolver` 的第二套下载/构建实现。

### 3.2 安装位置：`build/tools/`

工具是**可重建的构建缓存**，放在 `build/` 下，不放 `.phprt/`，也不放进产品树：

```text
build/
├── autoconf/
│   └── autoconf-2.72/              # 源码解压目录（现有 build/<name>/<stem>）
├── tools/
│   └── autoconf-2.72/              # 工具安装前缀
│       ├── bin/autoconf
│       └── .phprt-installed
└── php/
    └── php-8.5.12/
```

- `layout::tools_dir(root)` 返回 `<build-dir>/tools`。
- `--build-dir` / `local_build_dir` / `global_build_dir` 同时决定工具位置。
- 本地默认 `./build/tools`；全局默认 `~/phprtbuild/tools`。
- 产品重建不删 `build/`，所以工具跨产品、跨系列复用。
- `rm -rf build`、换 `--build-dir`、切全局/本地会丢失工具，下次重编；这是
  “可重建缓存”的明确语义，文档必须写清楚。
- `build/tools` 是保留目录，不和 `build/<name>/<stem>` 混用。

约束：

- Tool recipe 不能声明库依赖 `needs`，只能声明 `tools`（工具依赖工具）；否则工具
  会绑定到某个产品的依赖树，无法共享。
- Tool recipe 不能有 `product` / `series` / `source` / `modes` / `bundled` /
  `components` / `targets` 等产品/组件字段。
- 工具可以依赖宿主 toolchain（C/C++ 编译器、make、sh）；这些由产品 toolchain 和
  `tools` 检查覆盖。

### 3.3 声明：统一 `tools`

删除 `host_tools`，所有构建期工具统一写 `tools`：

```python
# 产品 profile
tools = [
    tool("gcc", min = "9"),
    tool("make", min = "4.0"),
    tool("patchelf", min = "0.13"),
    "ar",
]

# 组件
tools = [tool("autoconf", min = "2.68")]

# dep
tools = [tool("nasm", min = "2.13"), "cmake"]
```

- `tool(name, min = "", probe = ["--version"])` 保留；字符串缩写只检查存在。
- `Target.tools` 类型从 `Vec<String>` 改成 `Vec<ToolSpec>`；`Profile.tools` 类型不变。
- `host_tools` 字段删除；旧 recipe 的 `host_tools` 在加载时报未知字段。
- `extend_profile(tools = ...)` 仍按名字合并：同名替换，新名字追加。
- feature 的 `requires.commands` 保持现状：它是选择阶段的宿主机命令门禁，不触发
  工具源码构建。

### 3.4 `{tool:NAME}` 展开

`{tool:NAME}` 只在该节点的 `tools` 包含 `NAME` 时展开：

- **built**：展开为工具安装前缀，如
  `build/tools/autoconf-2.72`；
- **host**：跳过该 env 项，让宿主的 `PATH` 自己找到命令。

这样：

```python
env = {
    "PHP_AUTOCONF": "{tool:autoconf}/bin/autoconf",
    "PHP_AUTOHEADER": "{tool:autoconf}/bin/autoheader",
}
```

- 用宿主 autoconf 时，这两个变量不注入，`phpize` / `buildconf` 从 PATH 找；
- 用构建的 autoconf 时，变量指向 `build/tools/autoconf-2.72/bin/...`。

宿主路径和版本因此不会进入环境，也不会进入签名。

### 3.5 解析

解析发生在下载源码之前，按**节点**进行：

```text
对每个节点的 tools：
  host 探测通过（存在且满足 min）     -> Host
  已有 build/tools 版本满足 min      -> Store（复用）
  catalog 有满足 min 的版本          -> Store（需要构建）
  都没有                             -> 报错
```

- 产品 profile 的 `tools` 对整次构建生效（所有节点共享）；节点的 `tools` 只对该
  节点生效。两处的同名工具取最严的 `min`。
- 同一节点的同名工具取最严的 `min`；不同节点可以解析到不同 provider。
- 一次构建里同名工具最多构建一个版本；多个节点需要不同版本时，按节点分别解析，
  但 `build/tools` 允许版本目录并存。
- 工具依赖工具时，先解析依赖工具，再解析消费工具；有环直接报错。
- catalog 选版本时取满足 `min` 的最高版本，结果确定。
- 报错要区分：
  - `宿主 autoconf 2.60 不满足 >= 2.68，catalog 也没有可用版本`
  - `宿主缺少 nasm，catalog 没有 nasm 的构建 recipe`
  - `catalog 里 nasm 最高 2.16.03，不满足 >= 2.17`

### 3.6 构建

- 工具步骤排在所有消费节点之前；多工具按依赖拓扑排序。
- UI 标签 `[tools] autoconf 2.72`；日志 `build/logs/tool-autoconf-2.72.log`。
- 工具节点的 `PATH` 前置已构建的工具依赖；消费节点的 `PATH` 前置本节点解析到的
  built 工具 `bin`。
- 工具产物检查、`.phprt-sig`、失败日志复用现有 `phprt_build`。
- 工具安装前缀写 `.phprt-installed`（或复用现有 marker 格式），记录版本和工具签名；
  它不是产品 state，也不是第二套状态系统。
- 工具不参与链接闭包、不参与 runtime 收库、不参与 rpath。

### 3.7 签名

- 节点签名新增 built 工具项：

  ```text
  tool=autoconf provider=store:2.72 sig=<tool-sig>
  ```

- 宿主工具不写版本/路径，也不改 env；所以宿主机 autoconf 升级不会触发重编。
- built 工具变化（版本、recipe、flags）只重编实际使用它的节点，不影响无关节点。
- Tool 不进入 `resolution_digest`；它不是版本解析的一部分。
- Tool 不进入 scheme digest；它不是用户选择。

### 3.8 state / lock / plan / downloads

- `state.json` 加可选 `tools`：

  ```json
  "tools": {
    "autoconf": { "provider": "store", "version": "2.72" },
    "cmake": { "provider": "host", "version": "4.3.2" }
  }
  ```

  只记录解析结果，不写进 `components` / `bindings`，不参与 scheme。
- `--lock` 复用 `state.tools` 里的 built 工具版本；host 工具不锁版本，换机器时重新
  探测。工具不在 catalog 时报错，不静默回退。
- `plan` 增加 `tools` 段，文本输出增加“构建工具”一节；`plan` 不因工具缺失而失败，
  只显示将用 host / 将构建 / 无法解析。
- `--downloads` 把本次闭包里声明、且 catalog 有源码的工具全部加入下载；它保持
  “不检查、不构建”的语义，方便离线预取。真正 `install` 仍然 host-first。
- `audit` 显示实际使用的工具 provider 和版本。

---

## 4. Toolchain

### 4.1 为什么单独一层

gcc / g++ / glibc 不是 Tool：

- 不能像 patchelf 一样下载一个可执行文件替换；
- 它们决定 ABI、C 标准默认值、glibc 符号版本；
- 它们的作用范围是整次构建，不是单个节点；
- 未来容器化要按它们选择完整构建环境。

因此 Toolchain 的交付物是：

- 探测宿主机实际环境；
- 按产品声明的版本档位选择一档；
- 把该档的兼容 CFLAGS/CXXFLAGS 合入构建环境；
- 记录实际环境；
- 不满足声明时报错；
- 为未来容器 catalog 留出同一份 `ToolchainFacts`。

### 4.2 声明

只在产品级声明，不下沉到 dep / component：

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
        toolchain_range(min = "14", cxxflags = "-std=gnu++17"),
    ],
    glibc = toolchain_range(min = "2.17"),
)

TARGET = product(
    name = "php",
    series = "7.4",
    ...
    toolchain = TOOLCHAIN,
)
```

规则：

- `min` 包含；`max` 按写出的段数包含：`max = "13"` 允许 13.x、拒绝 14.0；
  `max = "2.40"` 允许 2.40.x、拒绝 2.41。
- 不写 `max` 表示开放上界；GCC 15、16 都会落到这一档，直到 recipe 再加更细的档。
- 一档都匹配不上时报环境不匹配，错误信息列出声明的区间和实际版本。
- 档位不能重叠；加载时校验，按 `min` 排序。
- `cflags` 只作用于 C，`cxxflags` 只作用于 C++；`glibc` 不带 flags。
- Rust 不根据版本号推导任何 flag；flag 永远是 recipe 数据。

### 4.3 探测与选档

探测命令：

| 项 | 命令 |
| --- | --- |
| gcc | `gcc --version` |
| g++ | `g++ --version` |
| glibc | `ldd --version`（GNU libc 行） |
| ld | `ld --version` |
| triple | `gcc -dumpmachine` |
| arch | `uname -m` |

记录结构：

```text
ToolchainFacts {
    gcc: "15.3.1",
    gxx: "15.3.1",
    glibc: "2.42",
    ld: "2.45.1",
    triple: "x86_64-linux-gnu",
    arch: "x86_64",
}
```

选档：

- 对每个声明的项，选包含实际版本的档；多项同时匹配时取 `min` 最大的那一档。
- 选中的档提供 `cflags` / `cxxflags`。
- 没声明 `gxx` 时不检查 g++，也不加 CXXFLAGS。
- 探测失败（命令不存在、输出解析不出）直接报错，不猜版本。

### 4.4 CFLAGS 合并

基础值来自现有 `build_env`：

```text
CFLAGS   = -O2
CXXFLAGS = -O2
```

如果 recipe 的 `env` 覆盖了 `CFLAGS` / `CXXFLAGS`，以 recipe 的值为基础，再追加
选中档的 flags：

```text
CFLAGS   = <基础 CFLAGS>   + " " + <选中档 cflags>
CXXFLAGS = <基础 CXXFLAGS> + " " + <选中档 cxxflags>
```

作用范围：

- 产品节点、dep 节点、component 节点；
- **不作用到 Tool 节点**：工具跨产品共享，不能带某个产品的兼容参数；
- folded 组件跟随产品节点，自然继承；
- standalone 组件（phpize）通过 `CFLAGS` 环境继承。

迁移时把 7.4 / 8.0 现有的 `env.CFLAGS` 移到 `gcc >= 14` 档：

```python
profile = PROFILE,   # 不再在 extend_profile 里写 CFLAGS
toolchain = TOOLCHAIN,
```

`-O2` 由 `build_env` 提供，档位只写 `-std=gnu17 -Wno-error=incompatible-pointer-types`。

### 4.5 签名

- 选中档的 `cflags` / `cxxflags` 进入 `env`，因此进入相关节点签名。
- **gcc / g++ / glibc 的原始版本号不进签名**。
- 同一档位内换编译器（14.1 → 14.2）不重编；跨档导致 flags 变化才重编。
- Toolchain facts 写进产品 marker，供 `plan` / `audit` 展示，不参与 scheme 和
  `resolution_digest`。
- 未来容器 digest 是否进签名，等容器方案落地时单独定；本文不预埋。

### 4.6 bundle 与容器

- bundle 继续只约束依赖版本线，不声明 toolchain。
- 产品声明的 gcc / glibc 范围覆盖整次构建；bundle 里选的依赖线必须在这个范围内
  可编，这是 recipe / bundle 的验收责任，不做节点级 toolchain 合并。
- 未来容器化时，容器 catalog 的每个条目提供同一套 `ToolchainFacts`（gcc、glibc、
  triple 等）；Rust 按产品声明的档位筛选，必要时由 CLI 指定容器条目。
- 只有一份 `ToolchainFacts`，不引入 `ContainerToolchain` 类型。构建执行器接收
  “环境”，本机还是容器对构建逻辑透明。
- 本文不实现容器 catalog、不引入 Docker/podman 依赖、不下载 gcc/glibc。

---

## 5. 与现有文档的关系

实施时要同步修改：

| 文档 | 改动 |
| --- | --- |
| `docs/PRODUCT.md` | `tools` 统一、删除 `host_tools`；工具最低版本与 host-first；`toolchain` 档位表；把“不能根据版本号自动推导”改成“recipe 显式声明档位，Rust 只选档”；7.4 / 8.0 CFLAGS 示例改为 toolchain 表 |
| `docs/CONTRACT.md` | `tool_target` 字段；`tools` 类型；`toolchain` / `toolchain_range`；删除 `host_tools` |
| `docs/BUILD.md` | 工具步骤顺序、`build/tools` 前缀、`{tool:}` 展开、CFLAGS 合并、日志标签 |
| `docs/CLI.md` | `--downloads` 包含工具源码；`plan` 的工具段；无新增 `--tool` 参数 |
| `docs/STATE.md` | 可选 `tools` 字段；不写 components/bindings |
| `docs/LOCK.md` | built 工具版本的 `--lock` 行为；host 工具不锁 |
| `docs/OUTPUT.md` | `plan --json` 的 `tools` / `toolchain` 字段 |
| `docs/FLOW.md` | 下载前解析工具；工具构建失败不写 state；add/remove 不接收工具参数 |
| `docs/DESIGN.md` | 第 14 节登记本文；四层数据补充 Tool / Toolchain |
| `docs/IMAGE.md` | 第 3 节的 toolchain catalog 与本文对齐：产品声明档位，容器条目提供 facts；旧的 `toolchain = "fedora-43"` 只作为容器选择的实现细节，不替代产品声明 |
| `docs/ADD-RECIPE.md` | 新增 Tool recipe、添加 toolchain 档位的步骤 |
| `AGENTS.md` / `README.md` | 简要说明 `recipes/tools/`、`build/tools/`、host-first、toolchain 档位 |

---

## 6. 迁移与实施顺序

本文不开发，实施分两阶段：

### Phase 1：Tool

1. `Kind::Tool`、`recipes/tools/` 加载、`tool_target(...)`。
2. `layout::tools_dir(root)` = `<build-dir>/tools`；工具源码仍在 `build/<name>/<stem>`。
3. 统一 `tools` 字段，删除 `host_tools`；`Target.tools: Vec<ToolSpec>`。
4. Tool resolver：节点级 host → store → catalog；工具拓扑排序；错误文案。
5. 构建：工具步骤、PATH、`{tool:}`、签名、日志、marker。
6. state / lock / plan / downloads / audit。
7. 迁移 autoconf：`recipes/dep/autoconf.star` → `recipes/tools/autoconf.star`；
   catalog `[autoconf]` 保留；组件 `tools` 改为 `tool(...)`；`PHP_AUTOCONF` 的
   host 情况跳过。
8. 测试和文档。

### Phase 2：Toolchain

1. `Toolchain` / `ToolchainRange` / `ToolchainFacts` 结构。
2. 产品 `toolchain(...)` 声明与校验。
3. 探测、选档、CFLAGS/CXXFLAGS 合并、marker、plan/audit。
4. 7.4 / 8.0 的 `env.CFLAGS` 迁到 `gcc >= 14` 档。
5. 测试和文档。

### 不在本文范围

- 容器化构建、容器 catalog、OCI 镜像；
- 下载或编译 gcc / glibc / binutils；
- cmake / nasm / patchelf 的源码 recipe（本次只把机制做出来，autoconf 是第一个真实用户）；
- 节点级 toolchain 声明和区间求交；
- 由 Rust 自动推导兼容 flags。

---

## 7. 文件改动清单

仅列预期改动，实施时以实际代码为准：

| 文件 | 改动 |
| --- | --- |
| `src/recipe.rs` | `Kind::Tool`、`ToolSpec`、`Target.tools`、删除 `host_tools`、`Toolchain` 结构、`recipes/tools` 加载、校验 |
| `src/tools.rs` | 从“检查”改为“解析 + provider” |
| `src/toolchain.rs` | 新增：探测、选档、facts |
| `src/layout.rs` | `tools_dir()`、`Kind::Tool` 的 label / build dir / downloadable |
| `src/build.rs` | 工具步骤、PATH、`{tool:}`、签名、CFLAGS 合并、toolchain marker |
| `src/main.rs` | 下载前解析、`--downloads` 工具源码、plan、state.tools、toolchain 检查 |
| `src/plan.rs` | `tools` / `toolchain` 输出 |
| `src/marker.rs` | built 工具签名项、toolchain 行、工具 marker |
| `src/state.rs` | 可选 `tools` 字段 |
| `src/lock.rs` | built 工具 pin |
| `src/audit.rs` / `src/manage.rs` | 工具与 toolchain 报告 |
| `src/render.rs` | `{tool:}` / 工具节点展示 |
| `recipes/defs.star` | `tool_target(...)`、`toolchain(...)`、`toolchain_range(...)`、删除 `host_tools` |
| `recipes/tools/autoconf.star` | 从 `recipes/dep/autoconf.star` 迁移 |
| `recipes/components/php/*.star` | `tools = [tool("autoconf", min = "2.68")]` |
| `recipes/dep/*.star` | `host_tools` → `tools` |
| `recipes/products/php/7.4.star` / `8.0.star` | `env.CFLAGS` → `toolchain` 档位 |
| `docs/*` | 第 5 节列出的同步 |

---

## 8. 验收

### 8.1 Tool 真实构建

1. **宿主缺失 → catalog 构建**
   本机没有 autoconf，catalog 有 2.72。`add php@8.5 'zlib=mode:shared' --offline`
   应构建 `build/tools/autoconf-2.72`，zlib 成功，`php -m` 有 zlib。
2. **宿主版本不匹配 → catalog 构建**
   临时在 PATH 前放一个假 `autoconf`，`--version` 报 2.60，其他参数直接失败；
   声明 `min = "2.68"`。解析应选 catalog 2.72；zlib 能编过就证明没有用假脚本。
3. **二次构建复用**
   再跑一次，显示“已构建”，不重复下载/编译。
4. **catalog 没有满足版本**
   临时把 `min` 改成 9.0，应在下载前报错，区分“宿主不满足”和“catalog 无可用版本”。
5. **宿主满足**
   临时给一个便宜节点声明 `tool("m4", min = "1.4")`；本机 1.4.21，`plan` 显示 host，
   不下载 m4 源码。
6. **无关节点不重编**
   改一个节点的工具要求，`plan` / 签名对比只影响该节点。

测试结束恢复临时 recipe 和假 PATH，不给正式业务依赖永久增加 patchelf 要求。

### 8.2 Toolchain

1. **PHP 7.4 + GCC 15**
   `plan php@7.4 --json` 显示 `gcc 15.3.1 → 档位 >=14`，最终 `CFLAGS` 含
   `-std=gnu17 -Wno-error=incompatible-pointer-types`；
   `install php@7.4 --targets cli` 应带上这些 flags 编译，而不是被版本门禁拒绝。
2. **档位不匹配**
   临时 recipe 声明 `gcc = [toolchain_range(min = "16")]`，本机 15.3.1 应报
   “不在声明区间”，不下载、不构建。
3. **签名**
   同一档位内换 gcc 版本不改变签名；跨档改变 flags 才重编相关节点。
4. **工具链不影响无关产品**
   PHP 8.5 没有 toolchain 声明，不受 PHP 7.4 档位影响。

### 8.3 回归

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets

phprt install php@8.5 --targets 'cli'
phprt add php@8.5 'zlib=mode:shared'
phprt remove php@8.5 zlib
phprt install php@8.5 --targets 'cli' --downloads
phprt plan php@8.5 --json
phprt audit php@8.5
```

重点确认：

- `add` 仍然立即执行必要的增量编译；
- `remove` 仍然按最终闭包收窄 state；
- `--downloads` 不写 state、不构建，但包含工具源码；
- 产品重建不再删掉工具；
- `[dep] autoconf` 不再出现，改为 `[tools] autoconf` 或直接复用宿主。

---

## 9. 未决与待实测

- PHP 7.5 / 8.6 的真实 gcc 档位和 flags，等上游源码和实测结果再填；本文只定机制。
- `gxx` 是否与 `gcc` 共用同一张表：当前设计分开，因为 C 和 C++ 的标准参数不同。
- host 工具版本是否需要 `--lock` 固定：当前建议不固定，host 工具属于环境，换机器
  重新探测。
- Tool store 是否提供清理命令：当前不做 `phprt clean`，用户删 `build/` 即可。
- patchelf 的源码 recipe：本次不写；等 rpath 后端确定后再决定是否需要。
- 容器 catalog 的 `ToolchainFacts` 字段和 digest 是否进签名，留给容器方案文档。
