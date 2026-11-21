# 宿主缺命令：安装提示

本文描述的行为已经实现（`src/host_commands.rs`、`recipes/images.toml`）。改行为时
同步 README、CLI.md、OUTPUT.md、FLOW.md、TOOLS.md、IMAGE.md、BUILD.md、
RPATH.md 和 DESIGN.md；本文与代码冲突时以代码实际行为为准。

宿主缺少构建命令时，phprt 不再只说“缺 make”，而是把本次方案缺的命令收齐，
按发行版拼成一条可以直接复制的安装命令。只提示，不执行。

---

## 1. 改造前的现象

缺命令时实测两条错误路径（Fedora 43）：

```text
$ phprt install php@8.5 --targets cli
error: 宿主缺少 make，catalog 没有 make 的构建 recipe

$ phprt plan php@8.5
error: 探测编译器 gcc 失败: 跑不起来 gcc: No such file or directory (os error 2)
```

三个问题：

| 问题 | 原因 |
| --- | --- |
| 只报第一个 | `prepare_order` 先跑 `tools::resolve(lenient = false)`，第一个失败就返回；`toolchain::resolve` 排在后面，根本没执行 |
| 没有安装办法 | 错误只说缺什么，不说怎么装；发行版包名和命令名不一定相同（`g++` 在 Fedora 是 `gcc-c++`） |
| `plan` 也失败 | `toolchain::resolve` 直接 `?`，和 TOOLS.md §3.8「plan 不因工具缺失失败」不一致 |

## 2. 边界

- **只提示，不执行。** 不跑 `sudo`、不装包、不改 PATH；宿主不满足就停，符合
  TOOLS.md §4.1。
- **覆盖“命令不存在”和“命令在但检查不过”。** 后者指工具的 `feature_check` 失败
  （如 Debian/Fedora 的 `perl` 缺 `FindBin`）。失败处理由工具 recipe 的 `on_fail`
  决定：`need_packages` 和缺命令一样进安装提示，`error_msg` 只打印、不进命令。
  `need_packages` 可以带一句 `reason`（如“当前 perl 模块不全，需要安装完整的 perl 包”），
  这句话会打在安装命令前面；不写就只报检查命令和它的原始输出。
  命令在但版本不满足、catalog 里没有可用版本、约束交集为空，仍按原错误报，
  不拼进安装命令：装包不一定能解决，拼进去反而会误导。
- **不推导库依赖。** 缺 `pkg-config` 是命令问题；缺 `libpng` 是依赖库问题，走 dep 解析，
  两者不混。
- **只覆盖本次方案用到的命令。** `cmake` 只在选了用 cmake 的依赖（libzip 等）时出现，
  不把整份 profile 里当前用不到的工具列进来。

## 3. 数据：`recipes/images.toml` 的 `[package_maps.*]`

包名映射是数据，不写进 Rust：发行版改包名、拆包、EOL 都只改这里，不用重新编译
phprt。这张表和镜像模式的自动安装共用一份，镜像部分见 [IMAGE.md](IMAGE.md) §3.1。

同一个文件里还有 `[images.*]` 镜像条目（镜像模式待实施，见 IMAGE.md）。宿主提示只读
`[package_maps.*]`。TOML 的裸键不能含 `+`，`g++` / `c++` / `clang++` 要加引号：

```toml
# 命令名 -> 发行版包名，按包管理器分组。
# 宿主安装提示和镜像自动安装共用；没写的名字不安装，只提示用户自行确认。

[package_maps.apt]
gcc        = "gcc"
"g++"      = "g++"
cc         = "gcc"
"c++"      = "g++"
clang      = "clang"
"clang++"  = "clang"
make       = "make"
ar         = "binutils"
ranlib     = "binutils"
ld         = "binutils"
pkg-config = "pkg-config"
patchelf   = "patchelf"
perl       = "perl"
m4         = "m4"
sh         = "bash"
cmake      = "cmake"
nasm       = "nasm"
autoconf   = "autoconf"

[package_maps.dnf]
gcc        = "gcc"
"g++"      = "gcc-c++"
cc         = "gcc"
"c++"      = "gcc-c++"
clang      = "clang"
"clang++"  = "clang"
make       = "make"
ar         = "binutils"
ranlib     = "binutils"
ld         = "binutils"
pkg-config = "pkgconf"
patchelf   = "patchelf"
perl       = "perl"
m4         = "m4"
sh         = "bash"
cmake      = "cmake"
nasm       = "nasm"
autoconf   = "autoconf"
```

规则：

- **键是命令名，不是声明名。** `toolchain` 的 `gxx` 归一成实际 CXX 命令名
  （`$CXX`，缺省 `g++`）；`CC` / `CXX` 可以指向 clang，所以 clang 也有条目。
- **未映射 = 不安装。** 不拼进安装命令，单列一行让用户自己确认包名。宁可少给一条
  命令，也不给出装不上的包名。
- `package_maps` 下只允许 `apt` / `dnf` 两张表；其它表名报错，不做通用抽象
  （同 IMAGE.md §3.1）。某张表可以不写，缺的表按全部未映射处理。
- 值必须是非空字符串；重复键由 TOML 自己报错。
- 文件不存在时映射为空，行为等同“全部未映射”，不报错（`--recipes` 指向别处时）。
- 现在只读 `[package_maps.*]`；`[images.*]` 的校验归镜像模式，实施时再加。
- 同一份 `Env` 还把 `id` / `family` 注入 recipe 的 `ctx["host"]`，recipe 据此选跟
  系统路径有关的构建参数（`dep/openssl.star` 的 `--openssldir`），见
  [CONTRACT.md](CONTRACT.md) 的 ctx 一节。

## 4. 包管理器识别

| 来源 | 匹配 | pm |
| --- | --- | --- |
| `/etc/os-release` 的 `ID` / `ID_LIKE` | `debian` `ubuntu` `linuxmint` `pop` `raspbian` | `apt` |
| 同上 | `fedora` `rhel` `centos` `rocky` `almalinux` `ol` | `dnf` |
| PATH | 有 `apt-get` | `apt` |
| PATH | 有 `dnf` | `dnf` |
| 都不匹配 | | 未知 |

- 先读 `/etc/os-release`，没有就读 `/usr/lib/os-release`；`ID` 和 `ID_LIKE` 都按空格
  拆开逐个匹配，大小写不敏感。
- 值可能带引号，去掉引号再比较。
- os-release 得不出结论时才看 PATH；两处都不匹配就是未知。
- **未知时不猜**：只列命令名，不给安装命令。
- 文本里显示的名字用 `NAME` + `VERSION_ID`（`Fedora Linux 43`），不用
  `PRETTY_NAME`（太长）。

## 5. 收集与判定

### 5.1 四类结果

| 分类 | 判定 | 进安装命令 | 例子 |
| --- | --- | --- | --- |
| 缺命令，只能装包 | 命令不在 PATH，catalog 里没有这个工具的 recipe | 是 | `make`、`cmake`、`m4` |
| 缺命令，可以自建 | 命令不在 PATH，catalog 有满足区间的 recipe | 否，直接构建 | `autoconf`、`patchelf` |
| 命令在但检查不过，能装包 | 命令在、版本也满足，但 `feature_check` 失败，`on_fail = need_packages` | 是（等同缺命令） | `perl` 缺 `FindBin` |
| 命令在但检查不过，装包无用 | 同上，`on_fail = error_msg` | 否，直接打印 | 自编 perl 缺模块 |
| 版本不满足 | 命令在，版本不在区间，catalog 也没有可用版本 | 否，单独列 | `make 3.8 < 4.0` |
| 数据问题 | catalog 最高版本不满足区间、约束交集为空 | 否，保持原错误 | `catalog 里 autoconf 最高 2.72，不满足 >= 9.0` |

### 5.2 收集时机

`prepare_order` 的流程：

```text
tools::resolve(..., lenient: true)   # 不在第一个失败处返回
toolchain::check(...)                # 缺命令返回 missing；档位不匹配仍是错误
汇总：Missing（进安装命令）+ Blocked（原错误）
install / add / remove               # 有 Missing 或 Blocked 就返回聚合错误
plan                                 # 不返回错误，把结果放进 Sim
```

- 收集范围 = 解析过程中每个节点实际的要求（非工具节点：`profile.tools` + 节点
  `tools`；工具节点：它自己的 `tools`），加上 `toolchain` 的 `gcc` / `gxx`。同名去重。
- 顺序：`gcc` / `gxx` 的实际命令名在前（按 `toolchain` 声明顺序），其余按字典序。
  结果确定，便于测试和比对。
- 工具节点也要用的命令（`make`、`gcc`）不单独展开，靠去重。
- 聚合只在解析阶段做；构建中途缺命令仍是原来的失败，那时环境不该再变。

### 5.3 feature 的 `requires.commands`

`feature_available` 在选择阶段单独检查命令，发生在解析之前，不进同一次聚合。
用同一个生成器输出单条命令的提示：

```text
error: dep:jpeg 的 feature simd 需要命令 nasm
Fedora Linux 43（dnf）上执行：
  sudo dnf install nasm
```

未映射的命令只留第一行，不追加安装命令。

### 5.4 不判定的场景

- `--downloads`：只取源码，不检查环境，不输出提示（保持 BUILD.md §7 的语义）。
- `plan` 的 toolchain 档位不匹配：仍然报错，不降级成提示。档位不匹配是 recipe 数据和
  编译器版本的真实冲突，装包不一定能解决。

## 6. 输出

### 6.1 install / add / remove

收集完再失败，一次给全：

```text
error: 宿主缺少构建命令：gcc g++ cmake make m4
这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。
Fedora Linux 43（dnf）上执行：
  sudo dnf install gcc gcc-c++ cmake make m4
```

命令顺序固定：编译器在前（按 `toolchain` 声明顺序），其余按字典序；包名按同样的
顺序去重。

工具 recipe 写了 `reason` 时，在“由系统包提供”那行后面多打一句：

```text
error: 宿主缺少构建命令：perl
这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。
  perl：当前 perl 模块不全，需要安装完整的 perl 包
Fedora Linux 43（dnf）上执行：
  sudo dnf install perl
```

有未映射的名字时追加一行：

```text
未映射：foo（请自行确认包名）
```

同时有“版本不满足”时，安装命令在前，不能靠装包解决的项在后：

```text
error: 宿主缺少构建命令：gcc g++ make
这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。
Fedora Linux 43（dnf）上执行：
  sudo dnf install gcc gcc-c++ make
另有 1 项不能靠装包解决：
  cmake 3.10 不满足 >= 3.20，catalog 也没有可用版本
```

只有“版本不满足 / 数据问题”时，输出和现在一致，不带安装命令：

```text
error: cmake 3.10 不满足 >= 3.20，catalog 也没有可用版本
```

识别不出包管理器时不给命令：

```text
error: 宿主缺少构建命令：gcc g++ make
这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。
未识别发行版，请用本机包管理器安装以上命令。
```

退出码仍是 1。

### 6.2 plan 文本

`plan` 不失败，在 `tools` 段之后、`build order` 之前加一段，只在非空时输出：

```text
host commands
  missing   gcc g++ cmake make m4
  install   sudo dnf install gcc gcc-c++ cmake make m4
```

- 有 `reason` 时在 `missing` 后面每个命令一行 `reason`：

  ```text
  host commands
    missing   perl
    reason    perl：当前 perl 模块不全，需要安装完整的 perl 包
    install   sudo dnf install perl
  ```

- 未知 pm：`install   未识别发行版，请用本机包管理器安装`。
- 有未映射项时多一行 `unmapped  foo bar（请自行确认包名）`。
- `toolchain` 段缺命令时显示 `(缺少)`：

```text
toolchain
  cc        gcc (缺少)
  cxx       g++ (缺少)
  triple    - / x86_64
```

### 6.3 plan --json

`Sim` 加 `host_commands`，没有缺失项时为 `null`：

```json
"host_commands": {
  "pm": "dnf",
  "missing": ["gcc", "g++", "cmake", "make", "m4"],
  "reasons": {},
  "unmapped": [],
  "install": "sudo dnf install gcc gcc-c++ cmake make m4"
}
```

- `reasons` 是“命令键 → 一句话说明”，只有工具 recipe 写了 `reason` 才非空：
  `"reasons": {"perl": "当前 perl 模块不全，需要安装完整的 perl 包"}`。

- 未知 pm：`"pm": ""`、`"install": ""`。
- `toolchain` 段加 `"missing": ["gcc", "g++"]`，为空数组表示编译器齐全。

## 7. 所有安装提示走同一个生成器

发行版名、包名映射、安装命令只有一份实现（`src/host_commands.rs`），三处都调用它：

| 位置 | 用法 |
| --- | --- |
| `src/recipe.rs` 的 `feature_available` | §5.3 的两行提示 |
| BUILD.md §7 | 说明“缺什么由 phprt 生成命令”，示例用本机的真实输出 |
| RPATH.md §3 / §7 的 patchelf 提示 | 实施 RPATH.md 时调用同一生成器 |

命令模板只有一份：

| pm | 命令 |
| --- | --- |
| apt | `sudo apt-get install <包...>` |
| dnf | `sudo dnf install <包...>` |

镜像模式用同一张映射表，但由宿主在容器里执行：`podman exec -u 0 <容器> <pm> install ...`，
不写 `sudo`，apt 先 `update`；模板见 IMAGE.md §4.3。本表只管宿主提示。

## 8. 文件清单

| 文件 | 改动 |
| --- | --- |
| `src/host_commands.rs` | 新增：os-release 解析、pm 识别、映射加载、收集、拼命令、聚合错误 |
| `src/tools.rs` | `Unresolved` 加分类，保留“缺命令 / 版本不满足 / 无可用 recipe”的区别 |
| `src/toolchain.rs` | 探测前先判命令是否存在；缺命令返回分类结果，不直接报错 |
| `src/main.rs` | `prepare_order` 始终 lenient；聚合与报错；`plan` 把结果传进 `Sim` |
| `src/render.rs` | `Sim` 加 `host_commands`；文本段；`toolchain` 段显示 `(缺少)` |
| `src/recipe.rs` | `feature_available` 改用生成器 |
| `recipes/images.toml` | 新增：只有 `[package_maps.apt]` / `[package_maps.dnf]` |
| `docs/*` | 第 7 节和 DESIGN.md 索引列出的同步 |

## 9. 验收

### 9.1 真实缺命令

本机（Fedora 43，缺什么列什么；下面是缺 `m4` 时的真实输出）：

```bash
cargo run -- plan php@8.5
```

```text
host commands
  missing   m4
  install   sudo dnf install m4
```

```bash
cargo run -- install php@8.5 --targets cli
```

```text
error: 宿主缺少构建命令：m4
这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。
Fedora Linux 43（dnf）上执行：
  sudo dnf install m4
```

退出码 1。

### 9.2 命令在但检查不过

Fedora 上只删掉 `perl` 总包（`sudo dnf remove perl`）时，`/usr/bin/perl` 还在、
`FindBin` 没了，属于“命令在但不能用”。用一个只在 `-MFindBin` 上失败的 `perl` 垫片
复现：

```text
host commands
  missing   perl
  reason    perl：当前 perl 模块不全，需要安装完整的 perl 包
  install   sudo dnf install perl
```

```text
error: 宿主缺少构建命令：perl
这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。
  perl：当前 perl 模块不全，需要安装完整的 perl 包
Fedora Linux 43（dnf）上执行：
  sudo dnf install perl
```

`plan` 的 `tools` 段里只剩一条 `perl (无法解析: 当前 perl 模块不全，需要安装完整的 perl 包)`，
不会再同时出现 `perl 5.42.3 (host)`：同名工具只要有一处解析不出，汇总列表就只留解析不出那条。

### 9.3 只缺一个

临时把 gcc 之外的命令补上，或反过来只把 `gcc` 从 PATH 里拿掉：安装命令只列缺的那几个，
不列已经有的。

### 9.4 装齐后不出现

```bash
cargo run -- plan php@8.5 | grep -c "host commands"   # 0
```

输出与改动前一致。

### 9.5 版本不满足不拼进命令

在 PATH 前放一个 `--version` 报 `3.8` 的假 `make`，`min = "4.0"`：

```text
error: 宿主 make 3.8 不满足 >= 4.0，catalog 也没有可用版本
```

不带安装命令。

### 9.6 单元测试

- os-release 文本 → pm：debian / ubuntu、fedora、`ID_LIKE` 多值、带引号、未知。
- 映射查找：`g++` → `gcc-c++`（dnf）、`pkg-config` → `pkgconf`（dnf）、未映射。
- 命令拼接：顺序（gcc 在前、其余字典序）、去重、未映射单列。
- `reason`：写在 `missing` 后面；只保留本次真的缺的命令，别的原因不串进来。
- 未知 pm 不产生 `install`。
- 宿主 ctx：`id` / `family` 注入 recipe。

### 9.7 容器/最小系统

容器里还有三件事它管不了（实测见 [IMAGE.md](IMAGE.md) §4.6、perl 拆包详见 §4.7）：

- **Debian 基础镜像没有 `sudo`**：容器里以 root 跑时去掉 `sudo`，宿主提示保持原样。
- **全新容器先 `apt-get update`**：否则 `apt-get install` 报 `Unable to locate package`。
  提示里不带 `update`，那是包管理器状态，不是缺命令。
- **Debian / Ubuntu 的 `perl-base` 有 `/usr/bin/perl` 却缺 `FindBin`**：现在由
  `openssl.star` 声明 `feature_checks = ["findbin"]`，检查写在 `recipes/tools/perl.star`
  （`-MFindBin -e1`），失败按
  `need_packages(["perl"], reason = "当前 perl 模块不全，需要安装完整的 perl 包")`
  并进安装提示，不会再拖到 openssl 的 `./Configure` 才失败。

## 10. 未决

- 包名会随发行版改名、拆包、EOL 漂移：改 `recipes/images.toml`，不改 Rust。
- 不做“自动安装”开关，也不提供 `--print-deps` 之类的第二入口。
- 同时装了多个包管理器的系统以 os-release 为准，不看哪个命令更“新”。
- 版本太低的场景不推导 `gcc-toolset-*` / `devtoolset-*` 之类的替代包名，只报错。
