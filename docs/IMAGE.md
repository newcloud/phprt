# 容器化构建与 OCI 镜像

**本文是待实施的方案，代码尚未实现。** 实施后按第 12 节的清单同步 README、
CLI.md、BUILD.md、CONTRACT.md 和 DESIGN.md；本文与代码冲突时以代码实际行为为准。

例外：第 3.1 节的 `[package_maps.*]` 已先用于宿主模式的安装提示，文件和加载器
随该功能落地，见 [HOST-COMMANDS.md](HOST-COMMANDS.md)。镜像条目 `[images.*]` 仍未实现。

## 1. 现在的缺口

构建直接跑在宿主机上，产物是 `.phprt/<产品>-<版本>/` 一棵树。它已经能跑，
但有两个缺口：

- **构建依赖宿主机**。换一台机器、上 CI，就要重新装 gcc / make / autoconf /
  perl / patchelf。`tools` 能检查缺什么，但不能把环境带过去。
- **树不能直接交付**。用户要手写 Dockerfile、自己拷库、自己试缺哪个 `.so`。

目标与 README「目标」一节一致：从一份声明直接得到能跑的最小镜像，而且 build
结束就能告诉用户下一条命令是什么。

现有的可搬移性是这件事的地基：运行时库已经收进 `lib/phprt`，rpath 全部是
`$ORIGIN` 相对路径（[RPATH.md](RPATH.md)），树整体平移不用重编。

## 2. 两个依赖，不要混

- **容器化构建**需要一个容器运行时来执行命令（podman / docker / kiri）。
  这是外部命令依赖，性质和 gcc 一样，不是可选项。怎么选、怎么拼命令、各家差在哪，
  见 [CONTAINER.md](CONTAINER.md)。
- **打包镜像**不需要 daemon 来做 `docker build`。用 Rust 的 `oci-spec` 出类型，
  `tar` + `flate2` 写层，`sha2` 算 digest。

第一件是执行环境，第二件是文件格式。混在一起会得出「用 Rust 写 OCI 所以不需要
容器」这种错误结论。

不过要补一句限定：首次出镜像仍要能读到 builder 镜像的内容（裁 runtime base、
做探针）。这一步可以起一次临时容器，也可以从 registry 取；结果按 digest 缓存后，
后续出镜像完全离线。所以准确的说法是：**不需要 daemon 来构建镜像，但第一次要能
访问那个镜像**。

## 3. 构建镜像是一条线

镜像不是写死的常量，而是和补丁号一样的数据：catalog 存、recipe 声明要求、
bundle 给默认、state 记实际选择。产品 recipe 里永远不出现 `debian:12`。

宿主构建不需要这一节；`[package_maps.*]` 由宿主安装提示和镜像模式共用，镜像条目
`[images.*]` 只在 `--buildbyimage`（或 `build` 出镜像）时用到。

### 3.1 catalog：`recipes/images.toml`

```toml
[images.fedora43]
image  = "quay.io/fedora/fedora:43"
digest = "sha256:40e0..."
pm     = "dnf"
gcc    = "15.3"
glibc  = "2.42"

[images.debian13]
image  = "docker.io/library/debian:13"   # 国内可换 docker.m.daocloud.io/…，digest 相同
digest = "sha256:9c6e..."
pm     = "apt"
gcc    = "14.2"
glibc  = "2.41"
packages = ["perl"]          # 命令在、包不完整时的兜底（Debian 的 perl-base 缺 FindBin）
# bootstrap = [              # 自动装包之前、以 root 在容器里按顺序执行
#   "sed -i 's|deb.debian.org|mirrors.tuna.tsinghua.edu.cn|g' /etc/apt/sources.list.d/debian.sources",
# ]

[images.ubuntu2604]
image  = "docker.io/library/ubuntu:26.04"
digest = "sha256:d66b..."
pm     = "apt"
gcc    = "15.2"
glibc  = "2.43"
packages = ["perl"]          # 同 debian13：perl-base 缺 FindBin

# 命令名 -> 发行版包名，按包管理器分组。完整内容、未映射规则和校验见
# HOST-COMMANDS.md §3；没写的名字认为基础镜像自带，装完由常规 tools 检查兜底。
[package_maps.dnf]
gcc        = "gcc"
"g++"      = "gcc-c++"
pkg-config = "pkgconf"
```

- 一个条目就是一条线，名字是 `debian13` / `fedora43` 这种稳定标识，和补丁号分开。
- `digest` 必填。可复现、签名、探针缓存都按它索引。
- `gcc` / `glibc` **手写 + 首次使用时探针校验**，不一致就报错，不静默采信。
  理由：纯探针生成的话，加一条镜像必须联网拉镜像，`--offline` 就用不了；
  纯手写不校验的话，tag 漂了或写错了，错误会拖到编译中途才暴露。
  `gcc` 写 major.minor（`15.3`），参与构建判断；`glibc` 只用于显示和运行下限提示，
  选镜像时告诉用户「产物要求目标系统 glibc >= 这个值」，不参与构建是否可用的判断。
- `pm` 只支持 `apt` / `dnf`；其它包管理器报错，不做通用抽象。
- `packages`（可选）：跟 phprt 自动装包一起装的额外包，按镜像条目的 `pm` 直接写
  发行版包名，不走 `[package_maps.*]`。用于“命令存在但包不完整”这类推导不出来的差异
  （Debian/Ubuntu 的 `perl-base` 缺 `FindBin`），以及容器里要预置的包。
  它和 `bootstrap` 只在创建容器时用一次，改了要重建容器才生效。
- `bootstrap`（可选）：创建容器后、以 root 在容器里按顺序执行一次的命令列表，
  用来改包源、加 GPG key。**通用机制，不分发行版**：换系统只加数据行，不改语法。
  约束见 §4.3。
- 旧条目保留规则同 catalog 补丁：升级某条线的 digest 时把旧条目留在文件里，
  `--lock` 只从这里取。
- 用户加镜像 = 在这里加一段并跑一次探针校验；加错、验不过由用户自己处理，
  仓库只对自带的条目负责。

`phprt list_oci_images` 打印这张表：

```text
$ phprt list_oci_images
线名        镜像                                  GCC     glibc  包管理
debian13    docker.io/library/debian:13           14.2    2.41   apt
fedora43    quay.io/fedora/fedora:43              15.3    2.42   dnf
ubuntu2604  docker.io/library/ubuntu:26.04        15.2    2.43   apt
```

表下一行固定说明：**glibc 是产物能运行的最低 glibc，不参与构建判断。**

### 3.2 产品声明：只声明 gcc / g++

选镜像的硬条件只有 `toolchain` 里的 gcc / g++ 区间：

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

tools = [
    tool("make", min = "4.0"),
    tool("perl", min = "5.20"),
    tool("m4", min = "1.4"),
    tool("pkg-config", min = "0.29"),
    tool("patchelf", min = "0.13"),
]
```

- gcc / g++（`gxx`）只写在 `toolchain`，不再写在 `tools`（见 [TOOLS.md](TOOLS.md)）。
- **不声明 glibc**。glibc 与编译无关，只决定产物能不能在目标系统上跑；
  它的值来自所选镜像的 catalog 条目，选镜像时展示给用户。
- dep / component 默认不声明 gcc；确实有已知不兼容时再写一条收窄约束，
  只求交、不带 flags（TOOLS.md §4.2）。
- `toolchain_range` 只管 gcc / gxx；`tool()` 的 `max` 是给 autoconf 这类工具版本
  用的，两者不混用。

### 3.3 bundle 默认

默认线写在 bundle，不靠「catalog 里恰好剩一条」隐式选：

```toml
[image]
line = "fedora43"
```

放在 `[image]` 表里，不占 bundle 顶层：顶层裸键已经用来写有独立版本线的组件，
不能和它抢名字空间。

### 3.4 state 与签名

安装成功后记录实际选择：

```json
"image": {
  "line": "debian13",
  "image": "docker.io/library/debian:13",
  "digest": "sha256:...",
  "gcc": "14.2",
  "glibc": "2.41",
  "container": {
    "name": "phprt-php-8.5-3f2a91c0",
    "id": "9b1d...",
    "created": "2026-11-22T00:45:00Z",
    "mounts": ["/home/z/blog:/home/z/blog"]
  }
}
```

- 新增可选字段，旧 state 没有时按 bundle 默认处理，schema 版本不升。
- 构建签名新增：原镜像的 **digest** 和「本地 / 镜像」两种构建方式。
  用 digest 不用线名，因为同一行的 tag 会漂；glibc 不单独进签名，digest 已经覆盖。
- 容器 id 不进签名：容器重建不改变原镜像的 libc，不应该触发全量重编。
  state 里只记名字和 id，下次直接复用，见 [CONTAINER.md](CONTAINER.md) 第 7 节。
- 影响：签名输入变了，已有安装树在升级后会判定签名变化并重编一次。
  这是必要的一次性成本，切换后端却不重编会拿到另一套 libc 编出来的产物。
- `--lock` 复用 state 里的镜像 digest。catalog 线按 catalog 查；用户镜像按本地
  `image inspect` 查，查不到就报错提示重新 pull，不偷偷换回默认线，规则同
  [LOCK.md](LOCK.md)。

### 3.5 求解与优先级

和现有五层优先级一致：catalog < recipe < bundle < state < CLI。

1. **默认**：宿主构建，不进入镜像模式。
2. **`--buildbyimage`**：进入镜像模式。
   - 空值：用 bundle 的 `[image].line`；没有默认线时报错。
   - catalog 线名（`debian13`）：用条目的 image / digest / pm / gcc / glibc。
   - 完整镜像 ref：当作用户自己 pull 的镜像；digest 从本地 `image inspect` 取，
     pm 和工具版本在容器里探测。没有 gcc 预检，容器起来后再验证。
3. **原镜像必须在本地**：三种来源都先 `image inspect`，没有就报错，不自动 pull。
4. **gcc 匹配**：产品声明了 `toolchain` 时：
   - catalog 线：用条目 `gcc` 匹配区间，不匹配直接报错并列出满足的线；
   - 用户镜像：无法预检，容器里跑 `gcc --version` 验证，不满足报错。

   产品没有 `toolchain` 声明时跳过这一条。
5. **glibc 只提示**：

   ```text
   镜像 debian12：gcc 12.2，glibc 2.36
   产物要求运行系统 glibc >= 2.36
   ```

   不阻塞、不需要 `--force`。用户选这个镜像就代表接受这个运行下限。

## 4. 容器化构建

### 4.1 一个长驻容器

`--buildbyimage` 进入镜像构建模式：从原镜像起一个不关的容器，phprt 在容器里探测工具
和编译，宿主负责查 catalog、下载、解压、装包。容器留在那里，下次 `add` / `remove` /
`update` 直接 `exec` 进去，不重装工具。

- 没有工具链镜像、没有 Dockerfile、没有 compose、不 `commit`。
- 原镜像必须由用户自己 pull；phprt 只检查本地有没有，不自动拉。
- 容器模式和容器记录写在 state 里，之后的命令不用再带 `--buildbyimage`。
- 完整流程、容器生命周期、挂载、打断参数见 [CONTAINER.md](CONTAINER.md)。

### 4.2 容器名

```text
phprt-<产品>-<系列>-<项目根 hash 8>
例：phprt-php-8.5-3f2a91c0
```

容器名在同一台机器上是全局的，带项目根 hash 才不会和别的项目撞名。用户不用记，
state 里有；phprt 结束时会打印删除命令。

### 4.3 容器里装什么

镜像本身只保证有基础系统；构建工具按 recipe 自动装。

1. **在容器里实际探测**：工具命令在不在、版本满不满足 `tool.min`、
   `feature_checks` 过不过。不是在宿主上猜镜像里有什么。
2. **分四类**：
   - 满足 → 不动；
   - 缺 / 版本不够 / 检查不过，但有包名映射 → 宿主 `exec -u 0` 装包；
   - 有 catalog recipe、没有可用包 → 宿主下载，编译时在 `build/tools` 里构建；
   - 既没有包映射也没有 recipe → 报错，点名补 `[package_maps.<pm>]`。
3. **装包**：

   ```bash
   podman exec -u 0 phprt-php-8.5-3f2a91c0 apt-get update
   podman exec -u 0 phprt-php-8.5-3f2a91c0 apt-get install -y --no-install-recommends gcc g++ make perl
   ```

   apt 先 `update`；dnf 直接 `install`。镜像条目的 `bootstrap` 在容器创建时跑一次，
   用来改包源、加 GPG key。装包不带 `sudo`。
4. **装完复检一次**：工具版本和 `feature_checks` 再跑一遍。仍缺就报错并列出缺什么，
   不再装第二轮。装好的工具留在容器里，下次不再装。

- 包名映射会随发行版改名、拆包、EOL 腐化，这是数据问题：改
  `recipes/images.toml`，不改 Rust。映射写错的表现为装不上或装完检查失败，
  错误里带包名。

### 4.4 运行时依赖

按 `phprt.toml` 的 `container_runtime` 选；没写时依次试 `podman` → `docker` →
`kiri`。都找不到时在下载源码之前报错，并列出每个候选的失败原因。
显式指定的运行时连不上时直接报错，不自动换下一个。

只用 `info` / `image inspect` / `inspect` / `run -d` / `exec` / `start` / `kill` / `rm`，
不 `build`、不 `commit`。完整规则（探测、挂载、用户、失败文案、测试矩阵）见
[CONTAINER.md](CONTAINER.md) 第 8 节。

### 4.5 容器里编的产物在宿主上不一定能跑

产物的 glibc 下限来自原镜像。宿主 glibc 更旧时，`.phprt/.../bin/php` 在宿主上
直接跑会报 `GLIBC_2.xx not found`。这不是 bug，是符号版本决定的：

- 宿主和原镜像同发行版同代际：本地 `myphp` 照常用；
- 宿主比原镜像旧：产物只当镜像素材，本地跑不了；
- 要在旧宿主上跑：用和宿主同代或更旧的原镜像。

这条要写进 `build` 的说明和报错，否则用户会以为是构建坏了。

### 4.6 裸镜像手工实测记录（2026-11）

实现镜像模式之前，用三个**未装任何工具链**的基础镜像手工验证了「缺什么、装什么、
装完能不能编过」。这节只记镜像事实和踩坑；自动化流程（长驻容器 + 阶段流水线）
见 [CONTAINER.md](CONTAINER.md)。

镜像条目见 §3.1：`fedora43`（quay）、`debian13` / `ubuntu2604`（docker.io，国内可换镜像站）。

**三个基础镜像自带什么**

| 镜像 | 自带 | 没有 |
| --- | --- | --- |
| `fedora:43` | `sudo` `dnf` `sh` `uname` `tar` `xz` | gcc、g++、ar、ranlib、make、cmake、m4、perl、pkg-config |
| `debian:13` | `apt-get` `sh` `perl`(5.40，perl-base) `uname` `tar` | `sudo`、binutils、make、cmake、m4、pkg-config |
| `ubuntu:26.04` | `apt-get` `sh` `perl`(5.40，perl-base) `uname` `tar` | `sudo`、binutils、make、cmake、m4、pkg-config |

**当时的手工验证**：起一个长驻容器，先在空镜像里跑一次 `install` 拿到缺命令和
安装命令，再装包、重跑 `install`。这也是现在要实现的形态：容器不关，装好的工具
留在里面；下面只保留实测到的事实。

实测结果（同一份 recipe，只换镜像）：编译 10 个节点全过，产物 `php -v` 为 8.5.12 (NTS)，
`php -m` 列出 43 项（含 `curl` / `openssl` / `pdo_sqlite` 等）；`dep:openssl` 的
`--openssldir` 按 `ctx["host"].family` 分别落 `/etc/pki/tls`（rpm）和 `/etc/ssl`（deb），
见 [CONTRACT.md](CONTRACT.md) 的 ctx 一节。

**三个坑（镜像模式的处理方式）**

1. **Debian / Ubuntu 基础镜像没有 `sudo`。** 提示里的 `sudo apt-get install` 原样粘贴会
   `sudo: not found`。容器里 `exec -u 0` 或 `RUN` 本身就是 root，不写 `sudo`；宿主提示保持
   原样（普通用户需要它）。
2. **全新容器必须先 `apt-get update`。** 否则 `apt-get install` 报
   `Unable to locate package`。这件事 §4.3 已经写进自动装包步骤；宿主机日常不需要。
   提示里不带 `update`，也不该带——那是包管理器状态，不是缺命令。
3. **`perl` 命令在、但缺 `FindBin` 模块。** Debian/Ubuntu 的 `perl-base`、Fedora 的
   `perl-interpreter` 都只带命令不带全部核心模块，openssl 的 `./Configure` 会直接失败。
   处理方式和实测细节见 §4.7。

**国内网络（实测）**

- `docker.io` 直连拉不动；`docker.m.daocloud.io/library/debian:13` 可以，digest 与
  官方一致（`sha256:9c6e38…`），所以条目里写官方名、注释写镜像站。
  `quay.io` 可用且快，Fedora 镜像直接从 `quay.io/fedora/fedora:43` 拉。
- 镜像内的 apt/dnf 源慢是**另一件事**：用镜像条目的 `bootstrap` 改源（数据，不是语法），
  它跑在自动装包之前，正好生效。
- phprt 自己的源码下载与系统源无关（URL 在 `recipes/catalog.toml`）。加速只有两条路：
  共享 `download_dir` 缓存，或给 catalog 换镜像地址。手工测试时把已有的
  `/home/z/testphp/downloads/*/*` 硬链接成平铺缓存（v3 的缓存是
  `<安装根>/.phprt/downloads/<文件名>`，不是按依赖分目录），10 个包 0 下载。
- phprt 二进制只依赖 `libc` / `libm` / `libgcc_s`，同代发行版可直接拷进容器跑，
  不需要在容器里重新编译。

**给镜像模式的验收清单**（基础镜像的缺包和装完能否编过已手工验证；流程本身待实现）

- 原镜像不在本地时报错，提示用户先 pull，不自动拉。
- 容器里探测出的包清单，和手工实测的缺包一致。
- 装完包后，工具版本和 `feature_checks` 通过。
- 容器里跑 `install` 全量构建成功；`php -m` 列出预期模块。
- 第二次 `install` / `add` 复用同一个容器，不重复装 gcc / make。
- 容器被删后重跑，自动重建并提示工具会重装。
- `plan --buildbyimage` 只列 `bootstrap`、包清单和依赖主线，不执行；`--offline` 不执行 `bootstrap`。
- 换原镜像后 `ctx["host"]` 变化会让对应节点重编（走 `inputs` 指纹 + 节点签名）。

### 4.7 已知拆包坑：perl（命令在、包不完整）

容器和宿主都实测到：`perl` 命令存在，`FindBin` 模块却不在，openssl 的 `./Configure`
第 15 行直接失败：

```text
Can't locate FindBin.pm in @INC (you may need to install the FindBin module) ... at ./Configure line 15.
BEGIN failed--compilation aborted at ./Configure line 15.
```

拆包情况（同一个坑的两种发行版形态）：

| 发行版 | `/usr/bin/perl` 来自 | `FindBin` 在 |
| --- | --- | --- |
| Debian 13 / Ubuntu 26.04 | `perl-base` | `perl` 包 |
| Fedora 43 | `perl-interpreter` | `perl-FindBin` 子包 |

`dnf repoquery --requires perl` 能看到 `perl-FindBin`，所以 `dnf install perl` 能补上；
Debian/Ubuntu 同理，`apt-get install perl` 即可。

phprt 的两层处理：

1. **宿主/最小系统**：`openssl.star` 声明
   `tool("perl", min = "5.20", feature_checks = ["findbin"])`，检查定义在
   `recipes/tools/perl.star`（`run = ["-MFindBin", "-e1"]`，失败
   `on_fail = need_packages(["perl"], reason = "当前 perl 模块不全，需要安装完整的 perl 包")`）。
   检查不过就并进统一安装提示：

   ```text
   error: 宿主缺少构建命令：m4 perl
   这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。
     perl：当前 perl 模块不全，需要安装完整的 perl 包
   Fedora Linux 43（dnf）上执行：
     sudo dnf install m4 perl
   ```

   宿主上也确实有这个问题：Fedora 43 只装了 `perl-interpreter`，phprt 会先拦下来，
   而不是拖到 openssl 编译中途。
2. **容器模式**：镜像条目 `packages = ["perl"]`（§3.1）保证自动装包时就是完整包，
   创建容器时装一次，之后一直留在容器里。

自检命令：

```bash
perl -MFindBin -e1                          # 退出码 0 才算可用
rpm -qf /usr/bin/perl                       # Fedora：perl-interpreter
dnf repoquery --requires perl | grep FindBin
```

实测（2026-11）：Fedora 43 宿主机 `sudo dnf install -y m4 perl` 后 `perl 5.42.3 (host)`、
`m4 1.4.21 (host)`，`plan` 的 `host commands` 段消失。

**同类坑的判断**：命令在 PATH 上只说明"有这个东西"，不说明包完整。再遇到类似情况
（`pkg-config` 找到的库缺开发文件、只有 `-dev` / `-devel` 包才有的头文件等），
优先给工具加一条 `feature_checks`（宿主也受益），`packages` 作镜像兜底；不要把发行版的
拆包规则写进 Rust。

## 5. 镜像

### 5.1 入口由 recipe 定

Rust 里不出现任何产品名。入口是产品声明，和 `tools` / `runtime` 一样写在
`product_profile(...)` 里：

```python
PROFILE = product_profile(
    ...,
    image = image(
        entrypoint = ["{tree}/sbin/nginx", "-p", "{tree}", "-c", "{tree}/etc/nginx.conf",
                      "-g", "daemon off;"],
        env = {"PATH": "{tree}/bin"},
        workdir = "/",
        ports = ["8080:80"],
    ),
)
```

- `entrypoint` 是 exec 数组。第一版只声明它，不声明 `cmd`、`stopsignal`、
  `healthcheck`。
- `ports` 写 `宿主:容器`，直接给 compose 用。写单个值时表示只暴露不映射。
  用 `8080:80` 而不是 `80:80`，是因为 rootless podman 绑不了 1024 以下的宿主端口。
- `env` 只放运行时需要的变量，构建期的 `CC` / `PHP_AUTOCONF` 不进来。
- 第一版镜像默认以 root 跑，和官方 PHP / nginx 镜像一致；`user` 字段先不做。
  非 root、`--read-only` 要处理属主和 tmpfs，是另一摊。

几个产品的 entrypoint：

- PHP CLI（选中 `cli` target）：`["{tree}/bin/php"]`，没有端口，不生成 compose；
- PHP-FPM（选中 `fpm` target；镜像支持待实施）：`["{tree}/sbin/php-fpm", "-F"]`，
  `ports = ["9000:9000"]`；
- nginx：`["{tree}/sbin/nginx", "-p", "{tree}", "-c", "{tree}/etc/nginx.conf",
  "-g", "daemon off;"]`；
- MySQL：`["{tree}/bin/mysqld"]`；
- CPython：`["{tree}/bin/python3"]`。

`entrypoint` 是数组不是脚本。要脚本就把脚本文件放进产品目录（和 `entry.sh`
一样），解释器由同一个 recipe 声明带进镜像。带不带 shell 是 recipe 的选择，
工具不替它决定。

注意区分两个「入口」：产品 profile 或选中 target 的 `entry` / `entry_template` 是
**宿主上的开发入口**（`myphp`），不进镜像；`image.entrypoint` 是镜像的入口。

### 5.2 配置：树内默认 + 镜像覆盖层

配置已经在树里，`post_install` 写的，直接进镜像，不另造机制。但树里的配置有两类
问题，必须覆盖一层：

- **开发向**：`php.ini` 里 `display_errors = On`、`error_reporting = E_ALL`，
  原样进镜像等于把开发配置发布到生产；
- **编译期绝对路径**：`extension_dir` 和 nginx 的 `--http-log-path` 都指向构建机的
  `.phprt/...`，树平移到 `/opt/...` 之后就失效了。

所以 `image` 声明多一个 `config`，值是「镜像内路径 → 产品目录里的文件名」，
只在打镜像时应用：同名覆盖，不同名新增。**宿主安装树一个字节都不变。**

```python
image = image(
    ...,
    config = {
        "etc/nginx.conf":          "nginx.image.conf",
        "etc/conf.d/00-image.ini": "image.ini",
    },
)
```

文件放在 `recipes/products/<产品>/` 下，和 `entry.sh` 同级，能 diff、能高亮。
模板支持 `{tree}`；PHP 的 `extension_dir` 用 `{extension_dir}`，由工具从安装树里
解析出实际目录后替换。

PHP 的 `image.ini`：

```ini
extension_dir = {extension_dir}
display_errors = Off
error_reporting = E_ALL & ~E_DEPRECATED
log_errors = On
```

nginx 的 `nginx.image.conf`：

```nginx
error_log stderr warn;
pid /tmp/nginx.pid;

events {
    worker_connections 1024;
}

http {
    include {tree}/etc/mime.types;
    access_log /dev/stdout;

    server {
        listen 80;
        root {tree}/html;
        index index.html;
    }
}
```

`include` 和 `root` 写绝对路径，不依赖 prefix 解析；`-p {tree}` 留给五个
`*_temp` 目录用。

覆盖方式就是挂载，路径固定：

```bash
-v ./php.ini:/opt/php-8.5.12/etc/php.ini:ro
-v ./conf.d:/opt/php-8.5.12/etc/conf.d:ro
-v ./nginx.conf:/opt/nginx-1.29.8/etc/nginx.conf:ro
```

不做「扫 `/config` 目录」这种 entrypoint 魔法，也不做 envsubst 模板渲染。
要模板就自己写个 entrypoint 脚本带进镜像，工具不掺和。

### 5.3 日志与可写目录

**日志一律走 fd，不落文件。**

nginx 的覆盖配置里 `error_log stderr;` + `access_log /dev/stdout;`，nginx 原生
支持这两个写法。这里不用官方镜像那种软链，原因很实际：我们的日志路径是编译期
绝对路径（`{tree}/var/log/...`），指向构建机的目录，镜像里根本没有那个落点，
软链无处可建。

PHP CLI 没有日志文件：`log_errors = On` 且 `error_log` 为空时，错误直接走 SAPI
日志，CLI 就是 stderr。所以 `image.ini` 里不设 `error_log`。

php-fpm 加进来时，在它的 `php-fpm.image.conf` 里把默认的文件路径改掉：

```ini
error_log = /proc/self/fd/2
pid = /tmp/php-fpm.pid
```

pool 的 `access.log` 默认不开，要开就 `/proc/self/fd/1`；`slowlog` 默认关。
规则和上面一致，只是多一份配置。

`var/log/` 在镜像里**保留成空目录**，不删。它是给用户自己的配置写文件日志时用的，
挂载覆盖后路径不变。

**可写目录**：

- nginx 的五个缓冲目录：`client_body_temp/`、`proxy_temp/`、`fastcgi_temp/`、
  `uwsgi_temp/`、`scgi_temp/`。它们在树里已经存在，保留原位，不改到 `/tmp`——
  改路径要动 conf，上游默认就是 prefix 下。
- `var/`（pid 或用户自己的日志）、`/tmp`（session、上传）。
- 第一版默认 root，这些目录天然可写。`--read-only` 时把这几个目录挂 tmpfs，
  这是用户的事，镜像里不声明。

### 5.4 文件清单：从入口反推

一棵装好默认组件的 PHP 树有一百多 M，其中 `include/`、`bin/phpize`、
`bin/php-config`、`phpdbg`、man、静态库，镜像都不需要。所以镜像文件集合要正向选，
不是 `cp -a`：

1. 入口二进制的 `NEEDED` 递归，得到必须的 `.so`；
2. `etc/conf.d/*.ini` 里的 `extension=` 指向的 `.so`，再递归它们的 `NEEDED`；
3. profile 声明的固定文件（`php.ini`、CA、时区、`/etc/passwd` 等）；
4. `image.config` 覆盖或新增的文件。

产出一份 manifest，每项是路径、sha256、来源、被谁需要：

```json
{ "path": "lib/phprt/libssl.so.3", "sha256": "...", "source": "elf", "needed_by": ["bin/php"] }
```

manifest 同时做三件事：决定镜像里放什么；校验（对每个 ELF 的 `NEEDED` 再核一遍，
缺一个就报错，**不需要起容器跑**）；可读（`plan --json` 里能看到清单和估算大小）。

这就是 README 那句「不手猜缺哪个 `.so`」的判据：靠算，不靠试。

最小镜像里**没有 `phpize`、`php-config` 和头文件**，所以不能在容器里编扩展。
要装扩展就重新 build 一个镜像，不是进容器 `phpize`。这条要写进说明，否则用户会
按官方镜像的习惯来用。

### 5.5 runtime base 必须和 builder 同源

树上每个 ELF 的 `NEEDED` 都有 `libc.so.6`，符号版本由构建时那套 glibc 决定。
runtime base 比它旧，二进制直接起不来。

所以默认路径是：**从同一个 builder 镜像裁 runtime base**，只留 glibc 家族、
CA 证书、`/etc/passwd` 之类。裁剪清单向容器里的包管理器要一次（`rpm -ql glibc` / `dpkg -L libc6`），
按 digest 缓存到 `.phprt/images/<digest>.base/`，不手写文件列表；缓存之后
出镜像不需要再访问镜像。

不要另找小镜像：alpine 是 musl，distroless 的 glibc 版本和你的 builder 无关，
两者都是赌。

三种情况：

1. catalog 里的线：照上面的裁，同源自不必说。
2. catalog 外的镜像：先探有没有包管理器。有就照裁；没有（scratch / distroless /
   极度精简）就报错，要求 `--runtime-base <镜像>`，不猜。
3. 显式给了 `--runtime-base`：跳过裁剪，但比对 libc 版本，比 builder 旧就报错。

### 5.6 树放哪

整体平移到 `/opt/<产品>-<版本>/`。rpath 全是 `$ORIGIN` 相对路径，所以**不用重编**；
安装树里也没有绝对路径（`php.ini` 不含 `extension_dir`，那是入口运行时给的）。

镜像的 `Env` 写死绝对路径，入口直接 `exec`，不靠 bash 的 `cd -P` 算位置。
runtime base 因此可以不带 shell。

### 5.7 OCI 输出与压缩

一次构建的产物是一个目录：

```text
.phprt/images/php-8.5.12/
├── image.tar        # OCI image layout
├── manifest.json    # 文件清单，可审计
└── compose.yml      # 只有声明了 ports 才有（见第 6 节）
```

拷走这个目录就是完整一套，tar 和 compose 不会对不上。`--tag` 改目录名，
`/` 和 `:` 换成 `-`。

`image.tar` 的结构：

- `oci-spec` 出 `ImageManifest` / `ImageIndex` / `Config` / `Descriptor`；
- 两层：base 层（裁剪结果）+ 树层（manifest 选出的文件与必要 `/etc`）；
- **层用 gzip 压缩**，mediaType 是 `application/vnd.oci.image.layer.v1.tar+gzip`，
  descriptor 的 digest 按**压缩后**的 blob 算；
- **外层 `image.tar` 不再压缩**：层已经压过，再压一层几乎不变小，只会让 `load`
  多一次解压；
- 产物 digest、镜像 digest 写进镜像 config 的 label（`org.phprt.*`），
  不往镜像里塞文件；
- `--load` 才导入 docker / podman；默认只出 tar。

读取 OCI layout 需要 Docker 25+ 或 podman。这条写进说明和报错。

**不生成 Dockerfile。** 树已经自包含，Dockerfile 只是把「拷文件」再描述一遍，
还多一层和一次 `docker build`。

### 5.8 可复现

同样的输入必须得到字节一致的 `image.tar`：

- tar 内按路径排序，`mtime = 0`；
- uid / gid 一律 0；
- mode 归一化：目录和带可执行位的文件 `0755`，其余 `0644`；
- 层 gzip 的头部 `mtime = 0`，压缩级别固定（6）；
- 符号链接按链接写，不跟成实体文件（`lib/phprt` 里全是这种链）。

验收方式是同一输入构建两次，`sha256sum` 相同。

## 6. 运行提示与 compose

build 的终点不是「产出 tar」，是「用户马上能跑」。所以 build 结束时打印下一条
命令，并按需要生成 `compose.yml`。

### 6.1 输出

```text
镜像：php:8.5.12
产物：.phprt/images/php-8.5.12/
  image.tar
  manifest.json
  compose.yml

运行：
  podman load -i .phprt/images/php-8.5.12/image.tar
  docker compose -f .phprt/images/php-8.5.12/compose.yml up
```

加了 `--load` 时，load 自动做掉，提示里只剩 `compose up`。没有 `ports` 的产品
不生成 compose，提示只给 `docker run --rm <镜像> ...`。

### 6.2 生成规则

按 `image` 声明判断，不硬编码产品名：

- 声明了 `ports` → 生成 `compose.yml`；
- 没有端口（PHP CLI）→ 不生成，只给 `docker run`。

PHP 的 `image` 入口随构建目标（`--targets`）变化：只选 `cli` 时没有端口，不生成
compose；选中 `fpm` 时入口是 `php-fpm`、声明 `ports = ["9000:9000"]`，才生成 compose。
`image` 声明仍由产品 recipe 决定，Rust 只读最终的 `ports`，不在代码里判断产品名或
target 名。

理由是 PHP CLI 不是长期服务，硬塞 compose 进去，`up` 要么立刻退出，要么卡在
交互式 REPL。等 php-fpm 加进来，它的 `image` 声明里有 `ports = ["9000:9000"]`，
compose 自然就有了。

### 6.3 内容

```yaml
# 由 phprt 生成，重新 build 会覆盖。
services:
  nginx:
    image: nginx:1.29.8
    ports:
      - "8080:80"
    restart: unless-stopped
    # 要改配置时取消注释，并把文件放到 compose 同目录：
    # volumes:
    #   - ./nginx.conf:/opt/nginx-1.29.8/etc/nginx.conf:ro
```

- `image` 来自 `--tag` 或默认的 `<产品>:<补丁>`；
- `ports` 原样来自 `image.ports`；
- 配置和数据**默认不挂**：挂宿主上不存在的文件会被 docker 建成目录，直接报错，
  和「最快跑起来」相反。用户要改再取消注释；
- `restart: unless-stopped` 只给服务，一次性命令用不着。

生成物会被覆盖，这和 `myphp` 入口一样。要长期维护的配置复制出去再挂。

### 6.4 边界

- 不生成 `healthcheck`、`depends_on`、多服务编排、`.env`；
- 一个产品一个服务；php + nginx + fpm 的组合留给用户自己写；
- compose 只认本地镜像，`up` 之前必须先 load，这两步是一对，`--load` 只是压成一条；
- 不生成 Dockerfile。

## 7. 命令与输出

```bash
phprt list_oci_images
phprt install php@8.5 --buildbyimage
phprt install php@8.5 --buildbyimage debian13
phprt install php@8.5 --buildbyimage docker.io/library/debian:13
phprt install php@8.5 --stop-after packages
phprt install php@8.5 --stop-after mainline
phprt plan php@8.5 --buildbyimage --json
phprt build php@8.5
```

- `list_oci_images` 只读 `recipes/images.toml`，不联网、不拉镜像；输出见 §3.1。
- `--buildbyimage` 进入镜像构建模式：
  - 空值：用 bundle 的 `[image].line`；没有默认线时报错。
  - catalog 线名：用条目里的 image / digest / pm / gcc / glibc。
  - 完整镜像 ref：当作用户自己 pull 的镜像，digest 从本地 `image inspect` 取，
    pm 和工具版本在容器里探测。
- **原镜像必须在本地**，没有就报错提示先 pull，phprt 不自动拉。
- 容器由 phprt 创建并长期保留；`state.json` 记容器名和 id，之后的
  `install` / `add` / `remove` / `update` 直接复用，不重装工具。容器没了就自动重建。
- 流程是四个检查点：`packages` → `mainline` → `download` → `extract`，然后在容器里
  编译。`--stop-after <检查点>` 在产出之后退出，退出码 0，不写安装 state。
- `--stop-after packages` 只打印装包命令；`--stop-after mainline` 写
  `.phprt/plan/<产品>-<系列>.json`（依赖主线 + 工具来源），宿主下次运行优先读它。
- 打断之后重跑同一条命令就继续，每个检查点做一次幂等检查。
- 镜像模式记录在 state 里；之后的命令沿用，不用每次带 `--buildbyimage`。
- `build` 总是镜像模式：没给 `--buildbyimage` 就用 bundle 默认线；已经装过且签名
  一致时跳过编译，直接出镜像。
- `--force` 对 `install` 和 `build` 都生效；`--runtime-base`、`--load`、`--tag`
  只对 `build` 生效。
- base 和 manifest 都命中缓存时，出镜像不需要网络，也不需要起容器。
- 默认 OCI tag 是 `<产品>:<补丁>`，例如 `php:8.5.12`；`--tag` 覆盖。
- 不加 `phprt image` / `phprt export` / `phprt run` 之类的第二条命令。

`plan --json` 新增 `image` 段：原镜像、digest、gcc、glibc、容器名和状态，以及能不能
复用；`builder` 段：每个工具的状态、要装的包。`plan --buildbyimage` 只探测，不装包、
不下载、不编译。

## 8. 失败模式

- `--buildbyimage` 的值既不是 catalog 线名、本地也没有同名镜像：报错，提示
  `phprt list_oci_images`，或直接给完整的 `镜像@digest`。
- 原镜像不在本地：报错，提示先 `podman pull` / `docker pull`；不自动拉。
- catalog 里没有满足产品 gcc 区间的线：报错，列出每条线的 gcc 和产品要求的区间。
- catalog 线名的本地镜像 digest / gcc 和条目对不上：报错，提示重新 pull 或条目过期。
- 用户给的镜像无法预检 gcc：容器里跑 `gcc --version` 验证，不满足报错。
- 镜像里没有包管理器：`pm` 写错或镜像不对，报错。
- 包映射缺失、包名写错：装包失败或装完 `tools` 检查报缺工具，错误里带包名；
  用户补 `package_maps` 或换镜像。
- 装完复检仍缺工具：报错并列出缺什么，保留容器，不再装第二轮。
- phprt 二进制在镜像里跑不起来（glibc 不够）：报错，提示换同代镜像。
- 没有容器运行时：下载源码之前报错。
- 容器创建失败或 `bootstrap` 非 0 退出：删掉半成品容器，报错带命令和输出。
- glibc 比目标系统新：**不报错**，只在选镜像和 `plan` 时提示运行下限。
- runtime base 的 libc 比 builder 旧：报错。
- manifest 校验缺 soname：报错，写清缺哪个、被谁需要。
- 镜像写出失败：不写 state 的镜像相关记录，安装状态保留，重跑只重做镜像。
- `compose.yml` 写出失败：镜像 tar 保留，报错提示路径和原因，不静默跳过。

## 9. 不变量

1. 产品 recipe 不写镜像名、base、镜像线；只声明 gcc 要求、入口和配置。
2. 镜像里不放 phprt、不放工具链、不放源码、不放头文件。
3. 镜像 digest 只由输入决定，不由时间、宿主路径、umask 决定。
4. 执行后端只换环境，不改语义；切后端必须重编（签名含 digest）。
5. runtime base 必须与 builder 同源，或显式指定并通过 libc 校验。
6. 镜像文件集合由 manifest 决定，不由人肉列表决定。
7. 镜像配置覆盖只作用于镜像，不改变宿主安装树。
8. `compose.yml` 是生成物，不手写维护；重新 build 覆盖。
9. glibc 不参与构建判断；它只说明产物的运行下限。

## 10. 不在范围

- 推 registry（第一版只出 tar）；
- 多架构、交叉编译（与 DESIGN.md 的非目标一致）；
- 自动 pull 原镜像；
- 生成工具链镜像、Dockerfile、compose；
- 镜像分层缓存优化（先正确，层共享是后面的事）；
- 「产物能在老系统跑」的兼容矩阵；
- `cmd` / `stopsignal` / `healthcheck` 等镜像字段；
- 非 root 运行和 `--read-only` 的属主处理；
- `healthcheck` / `depends_on` / 多服务 compose；
- zstd 压缩、`--compression` 参数；
- 在镜像里放 phprt 或构建工具。

## 11. 落地顺序

每步独立验收，都不依赖下一步：

1. **image catalog + 原镜像选择 + 记录**。不碰编译器。验收：
   `phprt list_oci_images` 输出表格；`--buildbyimage` 空值 / 线名 / 完整 ref 都能
   解析；原镜像不在本地时报错且不自动 pull。
2. **容器创建与复用**。验收：首次创建、`bootstrap` 执行一次；第二次命令直接 `exec`；
   `kill` / `rm` 后自动重建；结束时打印删除命令。
3. **plan 进容器 + 装包**。验收：空镜像里给出缺包清单；宿主装包后复检通过；
   工具分成 `ok` / `package` / `source` / `error` 四类。
4. **检查点与主线文件**。验收：`--stop-after packages` 只打印装包命令；
   `--stop-after mainline` 写出主线文件；手动作业后重跑能续上；签名含原镜像 digest。
5. **manifest + 配置覆盖 + 静态校验**。纯宿主侧。验收：缺文件时报错而不是生成
   坏镜像；`image.config` 只改镜像不改树。
6. **OCI tar 输出**。验收：`podman load` 后能跑，两次构建字节一致，层是 gzip。
7. **compose + 运行提示**。验收：`docker compose up` 一条命令起服务，
   `podman logs` 能看到 nginx 的 access log。
8. **`phprt build` 串起来**，入口从 recipe 读。验收：新机器一条命令出镜像，
   并打印下一步命令。

第 1 步可以完全离线做，最容易先对齐语义。

## 12. 实施时的改动清单

| 文件 | 改动 |
| --- | --- |
| `recipes/images.toml` | 镜像 catalog：`[images.*]` 条目、`[package_maps.*]` 补全容器用到的工具 |
| `src/toolchain.rs` | 读镜像 catalog、按 gcc 区间选线 |
| `src/container.rs` | 新增：运行时探测、容器创建 / 复用 / 重建、`exec`、挂载集合、属主探针，见 [CONTAINER.md](CONTAINER.md) |
| `src/config.rs` | `container_runtime` 解析 |
| `src/state.rs` | 记录原镜像 ref / digest、容器名 / id / 挂载 |
| `src/build.rs` | 签名加入原镜像 digest 与「本地 / 镜像」方式；容器内走本地后端 |
| `src/main.rs` | `--buildbyimage` / `--stop-after`；检查点编排；`build`、`list_oci_images` |
| `src/render.rs` | `plan --json` 的 `image` / `builder` 段 |
| `crates/image/` | 新增：manifest、ELF `NEEDED` 解析、base 裁剪、gzip 层、OCI 写出 |
| `src/image.rs` | 新增：从 profile 的 `image` 声明生成 OCI config 和文件覆盖 |
| `src/compose.rs` | 新增：按 `ports` 生成 `compose.yml` 和运行提示 |
| `recipes/products/*/_profile.star` | `toolchain(...)`、`image(...)`（不再写 `libc`） |
| `recipes/products/*/*.image.conf` / `*.ini` | 镜像配置模板（5.2） |
| `recipes/bundles/*/<系列>.toml` | `[image].line` |
| `Cargo.toml` | `oci-spec`、`tar`、`flate2`、`sha2` |
| 文档 | 本文；[CONTAINER.md](CONTAINER.md)；DESIGN.md §14；CLI.md 的 `build`、`list_oci_images`、`--buildbyimage`、`--stop-after`、退出码；BUILD.md 加镜像模式说明；CONTRACT.md 的 `toolchain` / `image` 字段与 bundle `[image].line`；PRODUCT.md 的 toolchain 说明；README「目标」一节改成已实现 |

## 13. 验收（实施后）

实现前已在裸镜像里手工验证过缺包清单和装完能否编过，踩到的三个坑
（无 `sudo`、`apt-get update`、`perl-base` 缺 `FindBin`）见 §4.6；下面是代码
实现后的验收命令。

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets

# 原镜像必须本地已有
cargo run -- install php@8.5 --buildbyimage docker.io/library/debian:12   # 报错：先 pull
podman pull docker.io/library/debian:13

# 一次跑完：建容器 → 装包 → 查 catalog → 下载 → 容器里编译
cargo run -- install php@8.5 --buildbyimage debian13
.phprt/php-8.5.12/bin/php -v
.phprt/php-8.5.12/bin/php -m | grep -E 'openssl|curl|pdo_sqlite'

# 复用：同一个容器，不重装 gcc
cargo run -- add php@8.5 swoole
cargo run -- install php@8.5
.phprt/php-8.5.12/bin/php -m | grep swoole

# 容器被删：自动重建并提示工具会重装
podman rm -f phprt-php-8.5-3f2a91c0
cargo run -- install php@8.5

# 中断执行
cargo run -- install php@8.5 --stop-after packages     # 打印装包命令
cargo run -- install php@8.5 --stop-after mainline     # 写 .phprt/plan/php-8.5.json
cargo run -- install php@8.5 --stop-after download
cargo run -- install php@8.5 --stop-after extract
cargo run -- install php@8.5                           # 进容器编译，续跑

# 主线文件可校验
jq '.tools' .phprt/plan/php-8.5.json

# 计划可校验
cargo run -- plan php@8.5 --buildbyimage --json | jq .image,.builder

# 一条命令出镜像
cargo run -- build php@8.5
podman load -i .phprt/images/php-8.5.12/image.tar
podman run --rm php:8.5.12 --version
podman run --rm php:8.5.12 -r 'echo PHP_VERSION;'

# nginx：一条命令起服务
cargo run -- build nginx@1.29 --load
docker compose -f .phprt/images/nginx-1.29.8/compose.yml up -d
curl -fsS http://127.0.0.1:8080/ | head -1
docker compose -f .phprt/images/nginx-1.29.8/compose.yml down

# 可复现：两次完整构建，tar 字节一致
cargo run -- build php@8.5 --tag php:test-a
cargo run -- build php@8.5 --tag php:test-b
sha256sum .phprt/images/php-test-a/image.tar .phprt/images/php-test-b/image.tar

# 层是 gzip，外层 tar 未压缩
tar -tf .phprt/images/php-8.5.12/image.tar | head
mkdir -p /tmp/phprt-oci && tar -xf .phprt/images/php-8.5.12/image.tar -C /tmp/phprt-oci
file /tmp/phprt-oci/blobs/sha256/* | grep gzip

# 配置覆盖只改镜像
grep -q 'display_errors = On' .phprt/php-8.5.12/etc/php.ini
podman run --rm php:8.5.12 -i | grep display_errors   # Off

# 镜像选择 / gcc 区间
cargo run -- build php@8.5 --buildbyimage fedora43
cargo run -- install php@7.4 --buildbyimage debian13    # gcc 14.2 落在 4.8..13 档
cargo run -- install php@7.4 --buildbyimage fedora43    # gcc 15.3.1 落在 >=14 档
cargo run -- build php@8.5 --buildbyimage no-such-line  # 报错：不在 catalog，提示 list_oci_images
```

## 14. 和现有文档的关系

- [DESIGN.md](DESIGN.md) 是设计主入口，本文挂 §14；四层数据模型、不变量、
  非目标以主文档为准。
- [BUILD.md](BUILD.md) 讲现有构建；容器化只是换执行环境，顺序和签名规则不变。
- [CONTAINER.md](CONTAINER.md) 讲容器运行时怎么选、怎么拼命令、各家差在哪。
- [RPATH.md](RPATH.md) 的 `$ORIGIN` 相对 rpath 是镜像里整树平移、不进 bash 算路径的前提。
- [CONTRACT.md](CONTRACT.md) 的 profile 字段表要加 `toolchain`、`image`；
  bundle 要加 `[image].line`。
- [TOOLS.md](TOOLS.md) 定义 `toolchain` 只声明 gcc / g++，以及 recipe 级约束的规则。
