# 构建工具与工具链（Tool / Toolchain）

状态：已实现（Tool 与 Toolchain 的宿主模式；用 OCI 镜像执行见
[BUILD-IMAGES.md](BUILD-IMAGES.md)）。
本文定机制、字段和验收，与代码不一致时以代码实际行为为准。

目标是把现在混在 `host_tools`、`profile.tools`、`Target.tools` 和普通 dep 里的
构建期工具拆清楚：

- **Tool**：patchelf、cmake、nasm、autoconf、bison、re2c 这类独立构建辅助程序。
  优先用宿主机；宿主不满足时从 catalog 取源码构建，装到 `.phprt/build/tools/`。用构建镜像
  时，下面的「宿主」都指实例 rootfs，见 §4.6。
- **Toolchain**：gcc / g++ 这类基础编译环境。产品声明版本档位，每档可以带兼容
  CFLAGS；Rust 只探测、选档、记录，不下载 gcc；执行环境由构建环境决定
  （[BUILD-IMAGES.md](BUILD-IMAGES.md)）。glibc / ld / binutils 不声明。

两者都不新建一套平行的构建管线：Tool 复用现有 dep 节点的下载、解压、构建、日志、
签名和跳过逻辑；Toolchain 只做环境探测和参数合并。

---

## 1. 实施前的现状与问题

以下保留作为设计背景：

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
- 没有按编译器版本分档的机制；glibc / ld 只影响运行，不影响构建，不需要声明。

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
| **Toolchain** | gcc、g++ | 产品 `toolchain(...)` | 不编译 | 否 |

判定规则：

- 只要它**会被构建命令执行**、且不是链接进产物的库，就是 Tool。
- 只要它决定**编译器**，就是 Toolchain。glibc / ld / binutils 不声明：它们跟随
  所选 gcc 所在的系统或镜像，不单独做版本判断。
- 不要只凭名字判断：`cmake` 是 Tool，`libcmake` 若是库则仍是 dep；`autoconf` 是
  Tool，`m4` 也是 Tool，但 `libxml2` 不是。
- Toolchain 默认不下沉到 dep / component。产品声明的 gcc 范围覆盖整次构建；
  确实存在已知不兼容时，dep / component 可以声明一条只收窄的 gcc 约束，
  只做断言、不带 flags（见 §4.2）。

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

内部加 `Kind::Tool`，节点 id 带版本：`tool:<name>@<version>`（同名多版本要能并存）。
它复用现有 dep 节点的：

- catalog 版本选择、下载、sha256 校验、解压；
- `configure` / `make` / `artifacts` 执行；
- 构建日志、`.phprt-sig` 跳过、增量复用；
- `--downloads`、`--offline`、`--lock` 的源码缓存。

不新建 `ToolStore`、`ToolResolver` 的第二套下载/构建实现。

### 3.2 安装位置：`<build-dir>/tools/`

工具是**可重建的构建缓存**，放在构建目录下（本地默认 `.phprt/build`），不放进产品树：

```text
.phprt/build/                       # 本地默认构建目录，跟随 local_internal_dir
├── autoconf/
│   └── autoconf-2.72/              # 源码解压目录（<build-dir>/<name>/<stem>）
├── tools/
│   └── autoconf-2.72/              # 工具安装前缀
│       ├── bin/autoconf
│       └── .phprt-installed
└── php/
    └── php-8.5.12/
```

- `layout::tools_dir(root)` 返回 `<build-dir>/tools`。
- `--build-dir` / `local_build_dir` / `global_build_dir` 同时决定工具位置。
- 本地默认 `./.phprt/build/tools`；全局默认 `~/phprtbuild/tools`。
- 产品重建不删构建目录，所以工具跨产品、跨系列复用。
- `rm -rf .phprt/build`、换 `--build-dir`、切全局/本地会丢失工具，下次重编；这是
  “可重建缓存”的明确语义，文档必须写清楚。
- `<build-dir>/tools` 是保留目录，不和 `<build-dir>/<name>/<stem>` 混用。

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
    tool("make", min = "4.0"),
    tool("patchelf", min = "0.13"),
    "ar",
]

# 组件
tools = [tool("autoconf", min = "2.68")]

# dep
tools = [tool("nasm", min = "2.13"), "cmake"]
```

- gcc / g++ 不写在这里，由产品的 `toolchain(...)` 声明；它们也永远不会从
  Tool catalog 解析，避免出现“下载 gcc”的第二条路径。加载时校验：`tools` 里出现
  `gcc` / `g++` / `gxx` / `cc` / `c++` 直接报错；`recipes/tools/` 也不允许这些名字。
- `tool(name, min = "", max = "", probe = ["--version"], feature_checks = [])`：`max` 是
  可选上界，语义与 `toolchain_range` 一致（按写出的段数比较）；不写表示不限制。
  `feature_checks` 是检查名，具体怎么检查写在 `recipes/tools/<name>.star`：

  ```python
  # 使用方：谁需要谁声明
  tools = [tool("perl", min = "5.20", feature_checks = ["findbin"])]

  # 工具 recipe：recipes/tools/perl.star
  tool_target(
      name = "perl",
      feature_checks = {
          "findbin": feature_check(
              run = ["-MFindBin", "-e1"],
              on_fail = need_packages(
                  ["perl"],
                  reason = "当前 perl 模块不全，需要安装完整的 perl 包",
              ),
          ),
      },
  )
  ```

  解析顺序固定：**命令存在 → 版本满足 → 执行 feature_checks**。检查只在宿主提供的
  命令上跑，catalog 构建出来的不跑。失败按 `on_fail` 处理：`need_packages` 把键并进
  统一安装命令，`error_msg` 直接打印。同名工具的检查名取并集；检查名在工具 recipe 里
  找不到时直接报错。

  `need_packages(names, reason = "")`：`names` 是命令键，`reason` 是给人看的一句话，
  打进装包提示（`perl：当前 perl 模块不全，需要安装完整的 perl 包`），同时作为该工具
  在 `plan` 里“无法解析”的原因。不写 `reason` 就只报检查命令和它的原始输出。

- catalog 里有同名版本的工具按版本构建；**没有版本的就是系统提供**，不下载、不编译，
  只读它的 `feature_checks`，recipe 里不写 `buildable` 之类的标记。

- `tool(name, min = "", probe = ["--version"])` 保留；字符串缩写只检查存在。
- `Target.tools` 类型从 `Vec<String>` 改成 `Vec<ToolSpec>`；`Profile.tools` 类型不变。
- `host_tools` 字段删除；旧 recipe 的 `host_tools` 在加载时报未知字段。
- `extend_profile(tools = ...)` 仍按名字合并：同名替换，新名字追加。
- feature 的 `requires.commands` 保持现状：它是选择阶段的宿主机命令门禁，不触发
  工具源码构建。

### 3.4 `{tool:NAME}` 展开

`{tool:NAME}` 只在该节点的 `tools` 包含 `NAME` 时展开：

- **built**：展开为工具安装前缀，如
  `.phprt/build/tools/autoconf-2.72`；
- **host**：跳过该 env 项，让宿主的 `PATH` 自己找到命令。

它用于“上游不接受 PATH、必须用环境变量指定工具路径”的少数场景。

PHP 不再用它：`PHP_AUTOCONF` / `PHP_AUTOHEADER` 删除。phpize 的 `autoconf` /
`autoheader` 直接走 PATH，而消费节点的 PATH 已经由执行器前置本节点解析到的
built 工具 `bin`；host 情况由宿主 PATH 兜底。工具 bin 的 PATH 前置不需要写在
recipe 的 `env` 里。

宿主路径和版本因此不会进入环境，也不会进入签名。

### 3.5 解析

解析发生在下载源码之前，按**节点**进行；同一名字允许解析出多个版本：

```text
对每个节点，先合并 profile.tools 与节点 tools（同名求交：min 取大、max 取小）；
工具节点例外，只用自己的 tools（见 §3.2）。然后：
  host 探测通过（存在且满足区间） -> Host
  已有 .phprt/build/tools 版本满足区间 -> Store（复用）
  catalog 有满足区间的最高版本    -> Store（需要构建）
  都没有                         -> 报错
```

- 产品 profile 的 `tools` 是产品 / 依赖 / 组件节点的基线要求，不是“整次构建只
  解析一次”；不同节点可以因此落到不同版本。工具节点不继承它：工具要跨产品共享，
  否则会被绑回某个产品的依赖树。例如节点 A 需要 autoconf `<= 2.69`，节点 B 需要
  `>= 2.70`，同一次构建会产出两个 autoconf，各自用在自己的编译命令里。
- 同一节点的同名工具只取一个版本；不同节点可以解析到不同 provider
  （host / store），同一次构建里可以混用。
- 一次构建允许同名工具多个版本：`.phprt/build/tools/<name>-<version>` 并存，按
  `(name, version)` 去重。
- 工具依赖工具时递归解析，同样允许多版本；有环直接报错。
- 同名约束求交为空时报错，并点名是哪个节点的哪条约束冲突。
- catalog 选版本时取满足区间的最高版本，结果确定。
- catalog 必须保留旧工具版本：一个工具在 catalog 里可以有多条线，每条线一个可选
  版本；节点用区间收窄到旧版时就从这里取。取不到就报错。例如：

  ```toml
  [autoconf]
  "2.71" = { version = "2.71", url = "...", sha256 = "..." }
  "2.72" = { version = "2.72", url = "...", sha256 = "..." }
  ```

  节点 A 写 `tool("autoconf", min = "2.68", max = "2.71")`、节点 B 写
  `tool("autoconf", min = "2.70")`，同一次构建会产出两个前缀，各自用在自己的
  构建命令里。
- 报错要区分：
  - `宿主 autoconf 2.60 不满足 >= 2.68，catalog 也没有可用版本`
  - `宿主缺少构建命令：nasm`（另附发行版安装命令）
  - `宿主 perl 在，但检查不过（findbin / open）：Can't locate FindBin.pm / open.pm…`
    （命令在、`feature_check` 不过，按缺命令处理）
  - `当前 perl 模块不全，需要安装完整的 perl 包`
    （`need_packages` 写了 `reason` 时，用它替代上面那条原始输出）
  - `catalog 里 nasm 最高 2.16.03，不满足 >= 2.17`
  - `节点 openssl 要求 autoconf <= 2.69，Profile 要求 >= 2.70，交集为空`
- 宿主缺命令、catalog 也没有可用 recipe 时，错误里附上按发行版拼好的安装命令；
  同一次解析的缺失项收齐再报，不只报第一个，见 [HOST-COMMANDS.md](HOST-COMMANDS.md)。

### 3.6 构建

- 工具步骤排在所有消费节点之前；多工具按依赖拓扑排序；同名多版本各自有独立步骤。
- UI 标签 `[tools] autoconf 2.72`；日志 `.phprt/build/logs/tool-autoconf-2.72.log`。
- 工具节点的 `PATH` 前置它自己依赖的工具 `bin`；消费节点的 `PATH` 前置**本节点
  解析到的那一版** built 工具 `bin`。
- 工具产物检查、`.phprt-sig`、失败日志复用现有 `phprt_build`。
- `.phprt/build/tools/<name>-<version>/.phprt-installed` 是**安装完成标记 + tool-sig**（构建
  成功且产物齐全后写入），
  用来判断这个前缀和当前 recipe / 编译器是否匹配。它是可重建缓存的清单，
  不进产品 marker，也不进 state 的产品身份。
- 工具不参与链接闭包、不参与 runtime 收库、不参与 rpath。

### 3.7 签名

- 节点签名新增 built 工具项：

  ```text
  tool=autoconf provider=store:2.72 sig=<tool-sig>
  ```

- `tool-sig` 只算和工具产物有关的输入：

  ```text
  id=tool:<name>
  version=<catalog 版本>
  url=<源码地址>
  sha256=<源码 sha256>
  compiler=<实际编译器> <版本> triple=<triple>
  command=<configure 全部展开后的程序 + 参数>
  command=<make / install / prepare 命令>
  env=CFLAGS=... CXXFLAGS=...        # 只放编译相关，白名单
  tool-dep=<name> <version> sig=<依赖工具 sig>
  ```

- 以下输入**不进** tool-sig：`PATH`、`PKG_CONFIG_PATH`、`LD_LIBRARY_PATH`、
  `MAKEFLAGS`、`CMAKE_BUILD_PARALLEL_LEVEL`，以及宿主执行时才追加的
  `CFLAGS` / `CPPFLAGS` / `LDFLAGS`。理由：工具没有 `needs`，库路径只可能来自
  内部依赖或宿主；内部依赖已经由 `tool-dep` 的 sig 覆盖。
- 宿主工具不写版本/路径，也不改 env；所以宿主机 autoconf 升级不会触发重编。
- built 工具变化（版本、recipe、命令、编译 env、依赖工具）只重编实际使用它的
  节点，不影响无关节点。
- `compiler` 取**实际编译工具的编译器**（产品 `CC` / `CXX`，缺省 `gcc` / `g++`），
  不能固定探测 `gcc`。
- 宿主模式下就是宿主编译器版本；用构建镜像时是实例里探测到的编译器版本。
  工具目录跨产品共享，不同编译器版本会触发工具重编；要避免抖动就把目录按
  toolchain key 分开（当前未做）。
- Tool 不进入 `resolution_digest`；它不是版本解析的一部分。
- Tool 不进入 scheme digest；它不是用户选择。

### 3.8 state / lock / plan / downloads

- `state.json` 加可选 `tools`，按版本列，不按名字做 key：

  ```json
  "tools": [
    { "name": "autoconf", "version": "2.72", "provider": "store" },
    { "name": "autoconf", "version": "2.69", "provider": "store" },
    { "name": "cmake", "version": "4.3.2", "provider": "host" }
  ]
  ```

  只记录解析结果，不写进 `components` / `bindings`，不参与 scheme；产品 marker
  不写工具版本。
- `--lock` 复用 `state.tools` 里 `provider = "store"` 的 built 工具版本，且优先于
  宿主；同一节点有多个锁定版本同时满足时，取满足区间的最高版本，保证结果确定。
  host 工具不锁版本，换机器时重新探测。锁定版本不满足当前区间、或不在 catalog 里
  时报错，不静默回退。
- `plan` 增加 `tools` 段，文本输出增加“构建工具”一节；同名多版本分别列出。
  built 工具标出谁需要它：`autoconf 2.72 (store) -> ...  for xdebug`，`build order`
  和工具节点详情也带 `for`；覆盖全部非工具节点时写 `all nodes`。`plan` 不因工具
  缺失而失败，只显示将用 host / 将构建 / 无法解析；缺命令时另给安装提示，
  见 [HOST-COMMANDS.md](HOST-COMMANDS.md)。
- `--downloads` 把本次解析到、需要构建的工具源码加入下载（含同名多版本）；宿主满足
  的工具不需要源码。它保持“不检查、不构建”的语义，方便离线预取。真正 `install`
  仍然 host-first。
- `audit` 显示实际使用的工具 provider 和版本。

---

## 4. Toolchain

### 4.1 为什么单独一层

gcc / g++ 不是 Tool：它们是**宿主机提供的编译环境**，只做版本适配，不下载、不构建。

- 不能像 patchelf 一样下载一个可执行文件替换；
- 它们决定 ABI、C 标准默认值；
- 它们的作用范围是整次构建，不是单个节点；
- 构建镜像时要按它们核对实例里的编译器。

glibc / ld / binutils **不声明**：它们跟随 gcc 所在的系统或镜像，版本一致兼容；
它们只影响编译出的程序能否在目标系统上运行，不影响能不能编。

因此 Toolchain 现在的交付物是：

- 探测宿主实际编译器（只探测声明了的项）；
- 按产品声明的版本档位选一档；
- 把该档的兼容 CFLAGS/CXXFLAGS 合入构建环境；
- 记录实际环境；
- 不满足声明时报错（不下载、不安装、不回退）；命令不存在时附发行版安装命令，
  见 [HOST-COMMANDS.md](HOST-COMMANDS.md)；
- 用构建镜像时，对实例里的编译器产出同一份 `ToolchainFacts`，见
  [BUILD-IMAGES.md](BUILD-IMAGES.md)。

Toolchain 不提供可执行文件，不进 PATH，不参与安装。

两者的边界：

| | Tool | Toolchain |
| --- | --- | --- |
| 宿主缺失 / 版本不满足 | 从 catalog 取源码构建到 `.phprt/build/tools/` 使用 | **停止**，报环境不满足；缺命令时附安装命令 |
| 来源 | 宿主或 catalog 源码 | 只有当前主机（将来可能是镜像） |
| 作用 | 单个节点的构建命令 | 整次构建的编译器版本与 CFLAGS |
| 是否可下载 | 是 | 否 |

判断标准：能被 `./configure` / `make` 执行的辅助程序是 Tool；决定编译器、ABI 和
C 标准默认值的是 Toolchain。

### 4.2 声明

只在产品级声明；默认不下沉到 dep / component：

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
    name = "php",
    series = "7.4",
    ...
    toolchain = TOOLCHAIN,
)
```

规则：

- 只有 `gcc` / `gxx` 两项；没有 `glibc` / `ld` / `binutils`。
- `min` 包含；`max` 按写出的段数包含：`max = "13"` 允许 13.x、拒绝 14.0。
  具体比较语义和 `tool(max = ...)` 共用一份实现。
- 不写 `max` 表示开放上界；GCC 15、16 都会落到这一档，直到 recipe 再加更细的档。
- 一档都匹配不上时报环境不匹配，错误信息列出声明的区间和实际版本。
- 同一声明项里的档位不能重叠；加载时校验。因此不存在“多项同时匹配”，
  选了哪一档是确定的。
- `cflags` 只作用于 C，`cxxflags` 只作用于 C++。
- Rust 不根据版本号推导任何 flag；flag 永远是 recipe 数据。
- **recipe 级收窄**：dep / component 默认不写 gcc。确实有已知不兼容时，可以写
  一条只收窄的约束（例如 `gcc = toolchain_range(min = "4.8", max = "13")`）：
  - 与产品区间求交集；交集为空时报错，并点出节点和两条约束；
  - 只做断言，**不提供 cflags**；flags 始终只来自产品档位；
  - 修改时要写明不兼容的原因。

### 4.3 探测与选档

探测命令：

| 项 | 命令 |
| --- | --- |
| gcc | `$CC --version`（默认 `gcc --version`） |
| g++ | `$CXX --version`（默认 `g++ --version`） |
| triple | 实际编译器 `-dumpmachine`（只记录） |
| arch | `uname -m`（只记录） |

记录结构：

```text
ToolchainFacts {
    cc: "gcc",
    gcc: "15.3.1",
    cxx: "g++",
    gxx: "15.3.1",
    triple: "x86_64-linux-gnu",
    arch: "x86_64",
}
```

选档：

- 只探测声明了的项；没声明 `gxx` 时不检查 g++，也不加 CXXFLAGS。
- 对每个声明的项，找包含实际版本的档。
- 选中的档提供 `cflags` / `cxxflags`。
- 命令不存在时汇总成安装提示（[HOST-COMMANDS.md](HOST-COMMANDS.md)）；命令在但输出
  解析不出、或版本不在任何档位里，直接报错，不猜版本。
- 宿主模式没有候选列表：不满足声明就是环境不可用，不下载、不安装。

### 4.4 CFLAGS 合并

基础值来自现有 `build_env`：

```text
CFLAGS   = -O2
CXXFLAGS = -O2
```

如果 recipe 的 `env` 覆盖了 `CFLAGS` / `CXXFLAGS`，以 recipe 的值为基础，再追加
选中档的 flags。最终顺序固定为：

```text
宿主 CFLAGS  ->  recipe/env CFLAGS  ->  选中档 cflags
```

即**档位 flags 永远在最后**。原因：GCC 15 默认 `gnu23`，档位里的 `-std=gnu17`
如果被宿主 `CFLAGS="-std=gnu23"` 顶掉，PHP 7.4 会重新编不过。宿主 CFLAGS 仍然
只用于执行、不进签名；档位 flags 在它之后追加，保证兼容参数不会被覆盖。

作用范围：

- 产品节点、dep 节点、component 节点；
- **不作用到 Tool 节点**：工具跨产品共享，不能带某个产品的兼容参数；
- folded 组件跟随产品节点，自然继承；
- standalone 组件（phpize）通过 `CFLAGS` 环境继承。

实现注意：现有 `build.rs` 对 dep 节点跳过 `profile.env`，档位 flags 不能只挂在
`profile.env` 上；必须单独注入 dep / component / product 节点的 env，否则依赖库
拿不到 `-std=gnu17` 之类的参数。

迁移时把 7.4 / 8.0 现有的 `env.CFLAGS` 移到 `gcc >= 14` 档：

```python
profile = PROFILE,   # 不再在 extend_profile 里写 CFLAGS
toolchain = TOOLCHAIN,
```

`-O2` 由 `build_env` 提供，档位只写 `-std=gnu17 -Wno-error=incompatible-pointer-types`。

### 4.5 签名

- 选中档的 `cflags` / `cxxflags` 进入 `env`，因此进入相关节点签名。
- **gcc / g++ 的原始版本号不进节点签名**。
- 同一档位内换编译器（14.1 → 14.2）不重编；跨档导致 flags 变化才重编。
- Toolchain facts 写进产品 marker，供 `plan` / `audit` 展示，不参与 scheme 和
  `resolution_digest`。
- 工具的签名另行处理：`tool-sig` 包含实际编译器版本，见 §3.7。
- 用构建镜像时，原镜像 digest 进节点签名；实例名不进，规则见
  [BUILD-IMAGES.md](BUILD-IMAGES.md) §7。

### 4.6 构建镜像

- 工具解析仍在同一份 plan 里完成；命令在实例内执行，环境变量和顺序不变。
- 实例里缺 make / perl / patchelf 等工具时，按 [HOST-COMMANDS.md](HOST-COMMANDS.md)
  的包名映射生成安装命令，在实例里执行；装完照常跑 `tools` 检查和
  `feature_checks`，版本不满足按现有逻辑报错。规则见
  [BUILD-IMAGES.md](BUILD-IMAGES.md) §6.1。
- bundle 继续只约束依赖版本线，不声明 toolchain。
- 产品声明的 gcc 范围覆盖整次构建；bundle 里选的依赖线必须在这个范围内可编，
  这是 recipe / bundle 的验收责任。
- 本文不下载 gcc，也不引入 docker / podman 依赖；实例由 `crates/buildimages`
  管理。

---

## 5. 与现有文档的关系

实施时已同步修改：

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
| `docs/ADD-RECIPE.md` | 新增 Tool recipe、添加 toolchain 档位的步骤 |
| `docs/HOST-COMMANDS.md` | 宿主缺命令时的安装提示、`[package_maps.*]`、包管理器识别、聚合与输出 |
| `AGENTS.md` / `README.md` | 简要说明 `recipes/tools/`、`.phprt/build/tools/`、host-first、toolchain 档位 |

---

## 6. 迁移与实施顺序

实施分两阶段（均已完成）：

### Phase 1：Tool

1. `Kind::Tool`、`recipes/tools/` 加载、`tool_target(...)`。
2. `layout::tools_dir(root)` = `<build-dir>/tools`；工具源码仍在 `.phprt/build/<name>/<stem>`。
3. 统一 `tools` 字段，删除 `host_tools`；`Target.tools: Vec<ToolSpec>`。
4. Tool resolver：节点级 host → store → catalog；工具拓扑排序；错误文案。
5. 构建：工具步骤、PATH、`{tool:}`、签名、日志、marker。
6. state / lock / plan / downloads / audit。
7. 迁移 autoconf：`recipes/dep/autoconf.star` → `recipes/tools/autoconf.star`；
   catalog `[autoconf]` 保留；组件 `tools` 改为 `tool(...)`；删除
   `PHP_AUTOCONF` / `PHP_AUTOHEADER`，phpize 从 PATH 找 autoconf。
8. 测试和文档。

### Phase 2：Toolchain

1. `Toolchain` / `ToolchainRange` / `ToolchainFacts` 结构。
2. 产品 `toolchain(...)` 声明与校验。
3. 探测、选档、CFLAGS/CXXFLAGS 合并、marker、plan/audit。
4. 7.4 / 8.0 的 `env.CFLAGS` 迁到 `gcc >= 14` 档。
5. 测试和文档。

### 不在本文范围

- 构建环境与运行镜像（见 [BUILD-IMAGES.md](BUILD-IMAGES.md)、
  [plan/RUNTIME-IMAGE.md](plan/RUNTIME-IMAGE.md)）；
- 下载或编译 gcc；
- cmake / nasm 的源码 recipe（autoconf、patchelf 已有 `recipes/tools/` 下的真实用户）；
- 节点级 CFLAGS 档位；节点级 gcc 约束只做断言，不带 flags；
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
   本机没有 autoconf，catalog 有 2.72。`install php@7.4 'zlib=mode:shared' --offline`
   应构建 `.phprt/build/tools/autoconf-2.72`，zlib 成功，`php -m` 有 zlib。
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
   PHP 8.1 ~ 8.6 只有基础档（`>= 9`），不受 PHP 7.4 的 `>= 14` 档位参数影响。
   临时删掉某个系列的 `toolchain`，该系列只跳过编译器版本检查，不加任何参数。

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

- PHP 7.5 的真实 gcc 档位和 flags，等上游源码和实测结果再填；PHP 8.6 按
  php-8.6.0 构建验收，只有基础档（`>= 9`）。本文只定机制。
- `gxx` 是否与 `gcc` 共用同一张表：当前设计分开，因为 C 和 C++ 的标准参数不同。
- host 工具版本是否需要 `--lock` 固定：当前建议不固定，host 工具属于环境，换机器
  重新探测。
- Tool store 是否提供清理命令：当前不做 `phprt clean`，用户删 `.phprt/build` 即可。
- patchelf 的源码 recipe：本次不写；等 rpath 后端确定后再决定是否需要。
- 包名映射会随发行版漂移：只维护验证过的条目，映射错了由工具检查兜底，详见
  [HOST-COMMANDS.md](HOST-COMMANDS.md) 和 [BUILD-IMAGES.md](BUILD-IMAGES.md) §6.1。
