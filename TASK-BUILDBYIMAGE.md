# 开发任务：在容器镜像里构建（`--buildbyimage`）

本文是独立任务书，描述要做的功能、用户怎么用、内部怎么实现。不依赖其它文档；
读完这一份就能开工。

状态：**待开发**。目前仓库里没有任何相关代码，镜像 catalog 文件
`recipes/images.toml` 已经有条目但只有宿主安装提示在用。

---

## 1. 目的

现在 phprt 在宿主上直接编译。这带来两个问题：

1. **宿主的工具链绑定产物。** 宿主 glibc 新，产物就要求新系统才能跑；想给老系统
   出一个 PHP，得自己找台老机器。
2. **宿主环境不可控。** 编译器版本、发行版、包名各不相同，同一份 recipe 在不同机器
   上结果不同，出了问题很难复现。

要做到的是：**用户给一个容器镜像，phprt 在里面从零装好构建工具，再把产品和所有组件
编出来。** 宿主只负责起容器和传目录，不参与编译。

具体目标：

- 一条命令从空镜像编出可用的 PHP：`phprt install php@8.5 --buildbyimage`。
- 第二次用同一个镜像时不重装工具链，只补新增的；加一个扩展（`add swoole`）不该再花
  一次装 gcc 的时间。
- 工具链镜像由 phprt 自己生成并记录，用户不用手写 Dockerfile。
- 原镜像由用户自己 `podman pull`；phprt 不联网拉镜像，避免在受限网络里卡住。
- 编译产物落在项目目录里，属主是当前用户，不是 root。
- 不引入常驻容器和逐条 `exec`：宿主只调用 `image inspect` / `build` / `run`。

非目标（这一版不做）：自动 pull 原镜像、自动装容器运行时、跨架构构建、
静态链接 phprt、Kubernetes / LXC / buildah、推 registry。

---

## 2. 用户使用方式

### 2.1 一条命令

```bash
podman pull docker.io/library/debian:13        # 用户自己做，phprt 不代劳
phprt install php@8.5 --buildbyimage
```

`--buildbyimage` 的值有三种写法：

| 写法 | 含义 |
| --- | --- |
| 不写值 | 用 bundle 里 `[image].line` 指定的默认镜像线；没有默认线就报错 |
| 写线名（`debian13`） | 用 `recipes/images.toml` 里 `[images.debian13]` 这条 |
| 写完整镜像 ref（`docker.io/library/debian:13`） | 当作用户自己准备的镜像，digest 现查，包管理器和工具版本在容器里现探 |

catalog 线名会带来预检：条目里写了 `gcc`，就能在动手之前判断它是否落在产品要求的
编译器区间里；用户自己给的镜像没法预检，只能等工具链镜像建好后再跑 `gcc --version`
验证，不满足就报错。

执行完，项目里多出这些：

```text
.phprt/php-8.5.12/                          安装树（和宿主模式一样）
.phprt/images/toolchain/php-8.5-debian13/20261121T153012/Dockerfile
.phprt/toolchains.json                      本地工具链索引
```

### 2.2 之后的命令不用再带参数

镜像模式会写进项目 state。装过一次之后：

```bash
phprt add php@8.5 swoole        # 自动在同一个工具链镜像里编译
phprt install php@8.5           # 同上
phprt remove php@8.5 xdebug
phprt update php@8.5
```

这些都不会重装 gcc / make。工具没变化就不生成新镜像、不生成新 Dockerfile。

### 2.3 复用与选择

同一个原镜像配同一个产品时，默认复用上次的工具链镜像：

```text
$ phprt install php@8.5 --buildbyimage
原镜像  docker.io/library/debian:13@sha256:9c6e…（本地已有）

发现可复用的工具链镜像：
  1) phprt-toolchain:php-8.5-debian13   gcc 14.2  make 4.4  perl 5.40   2.1G  3 天前
  2) phprt-toolchain:php-8.5-fresh-7c21 gcc 14.2  make 4.4              1.9G  20 天前
  3) 不用旧镜像，从原镜像重新装

本次还要补：cmake
选择 [1]:
```

选择顺序固定为：

1. 命令行给了 `--toolchain <tag>` → 用它；
2. state 里记过、镜像还在本地、仍兼容 → 直接用，不提问；
3. 只有一条兼容 → 自动用，打一行提示；
4. 多条兼容 → 列出来让用户选；
5. 没有兼容的 → 从原镜像新建，不提问。

“兼容”= 原镜像 digest 和架构一致、gcc 落在产品要求的区间、镜像还在本地、
缺的工具能继续累加。不兼容的也列出来，标注原因，不能选。

非交互环境（没有 TTY、CI）能自动选就选，选不出来直接报错并列出候选，不阻塞。

两个开关：

- `--toolchain <tag>`：指定用哪一个，不匹配会报错而不是悄悄换别的。
- `--no-accumulate`：不从旧镜像接着装，直接从原镜像装一套新的。用新 tag
  `phprt-toolchain:<产品>-<系列>-<原镜像标识>-fresh-<hash 8>`，`hash` 由包清单和
  bootstrap 命令算出。同一个包清单得到同一个 fresh tag，可以复用；旧镜像不动。

### 2.4 分段执行

想先看缺什么、或者下载必须走宿主网络时，把流程切成两段：

```bash
# ① 容器里只探测工具，不下载、不编译；标准输出就是状态文件
phprt plan php@8.5 --buildbyimage --json > .phprt/plan/php-8.5.json

# ② 宿主补全（catalog 查版本、下载、解压），再进工具链镜像只编译
phprt install php@8.5 --buildbyimage --from-plan .phprt/plan/php-8.5.json
```

- 第 ① 步退出码 0，不写安装 state，输出可供人读也可以直接改。
- 第 ② 步读这个文件，宿主把 `sources` 补上（版本、URL、sha256、解压路径），容器里
  只读这份数据，不再查 catalog、不再下载。
- 一次跑完时也会写这个状态文件，只是不停下来，方便事后核对。

### 2.5 `plan` 的输出

`plan --buildbyimage` 在容器里跑，只到工具状态：

```text
builder  phprt-toolchain:php-8.5-debian13（复用）
install  apt-get update && apt-get install -y --no-install-recommends cmake
```

`plan --json` 多出两段：`image`（原镜像、digest、gcc、glibc、工具链 tag、能否复用）
和 `builder`（每个工具的状态、要装的包、Dockerfile 内容）。`plan` 不写 Dockerfile、
不建镜像。

### 2.6 报错长什么样

原镜像不在本地：

```text
error: 本地没有 docker.io/library/debian:13@sha256:9c6e…
phprt 不自动 pull。先执行：
  podman pull docker.io/library/debian:13
```

工具既没有包映射也没有 catalog 源码：

```text
error: 工具 patchelf 在工具链镜像里不可用，也没有可用的包映射
在 recipes/images.toml 的 [package_maps.apt] 里补 patchelf，或给 catalog 加一行
```

工具链镜像和原镜像对不上：

```text
error: phprt-toolchain:php-8.5-debian13 基于 debian:13@sha256:aaaa…，
当前原镜像是 sha256:bbbb…；不能累加。
加 --no-accumulate 从原镜像新建，或换回原来的原镜像。
```

phprt 二进制在镜像里跑不起来：

```text
error: phprt 在 docker.io/library/debian:12 里跑不起来：GLIBC_2.38 not found
换 glibc 不低于宿主的原镜像。
```

---

## 3. 原理

### 3.1 两类镜像

| 对象 | 谁产生 | 作用 |
| --- | --- | --- |
| 原镜像（base） | 用户 `podman pull` / `docker pull` | 只读底座；phprt 只检查本地有没有 |
| 工具链镜像（toolchain） | phprt 生成 | 原镜像 + gcc / make / perl 等构建工具，可复用、可累加 |
| Dockerfile | phprt 每次生成 | 给人看的记录；phprt 不解析、不复用以外的用途 |

默认 tag：

```text
phprt-toolchain:<产品>-<系列>-<原镜像标识>
例：phprt-toolchain:php-8.5-debian13
    phprt-toolchain:php-8.5-custom-9c6e38a1
```

`<原镜像标识>` 是 catalog 线名；用户直接给镜像 ref 时用 digest 前 8 位。换原镜像就是
换 tag，不会覆盖别的镜像。

### 3.2 一次跑完的流程

```text
宿主
 1. image inspect 原镜像；本地没有就报错，不 pull
 2. 起容器跑 phprt plan
      镜像 = 已有可复用的工具链镜像；没有就用原镜像
      挂载 = 项目根、build/、recipes 只读、phprt 二进制
      → 工具状态 + 包清单 + Dockerfile 内容
 3. 需要装新工具时：写 Dockerfile，调用 build
 4. 起工具链镜像的容器跑 phprt install
      容器里就是本地模式：catalog 检查、下载、解压、编译、安装、写 state
 5. 退出码、日志、state 通过挂载回到宿主
```

最关键的一条：**phprt 自己进容器跑，不是在宿主逐条 `exec` 命令。** 容器里的 phprt
认为自己在本地构建，构建执行器（`crates/build`）一行都不用改。宿主侧只做四件事：
检查镜像、构建镜像、起容器、传挂载，完全不关心产品是 PHP 还是 nginx。

为了避免递归，容器内用环境变量 `PHPRT_IN_CONTAINER=1` 标记，内部强制走本地后端。

### 3.3 挂载与路径

```text
项目根                     读写（.phprt、build 默认都在里面）
internal_dir / build_dir   读写（配到项目根外时补挂）
download_dir               读写（一次跑完时容器要下载）
recipes                    只读
phprt 二进制               只读，挂到容器内固定路径（如 /phprt）
```

路径保持宿主绝对路径，不做「宿主路径 → 容器路径」翻译。这样日志、state、错误信息
在两边是同一份字符串，用户不用做心算。

### 3.4 工具链镜像里装什么

不维护固定包清单，全部从 recipe 推导：

1. 收集命令名：`toolchain` 的 `gcc`（以及归一后的 `g++`），加上所有节点 `tools` 里
   声明的命令（make、perl、cmake、nasm……）。
2. 在容器里逐个探测：命令在不在、版本是否满足 `min`、`feature_checks` 过不过。
   不在宿主上猜镜像里有什么。
3. 分四类：

   | 状态 | 含义 | 后续 |
   | --- | --- | --- |
   | `ok` | 命令在、版本够、检查通过 | 不动 |
   | `package` | 缺 / 版本不够 / 检查不过，但有包名映射 | 写进 Dockerfile 装包 |
   | `source` | 没有可用包，但有 catalog 源码 | 宿主下载，编译时在 `build/tools` 里自建 |
   | `error` | 两样都没有 | 报错，点名去 `[package_maps.<pm>]` 补 |

4. 生成 Dockerfile 并构建；构建完在成品镜像里把版本和 `feature_checks` 再跑一遍，
   防止包源给了低版本。

`package` 里有一个典型例子：Debian / Ubuntu 的 `perl-base` 提供 `/usr/bin/perl`
但缺 `FindBin` 模块，openssl 的 `./Configure` 会直接失败。这类“命令在但检查不过”
必须按缺包处理，装完整包，而不是当成 `ok`。

生成的东西长这样：

```dockerfile
FROM docker.io/library/debian:13@sha256:9c6e…
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc g++ binutils make m4 perl pkg-config
```

apt 先 `update`；dnf 直接 install。镜像条目的 `bootstrap` 命令排在装包之前，用来改
包源、加 GPG key；它只在镜像模式、以 root 执行，非 0 退出立刻中止，`--offline` 时
不跑。

### 3.5 累加

复用一个已有工具链镜像时，新 Dockerfile 从它接着装：

```dockerfile
FROM phprt-toolchain:php-8.5-debian13
RUN apt-get update && apt-get install -y --no-install-recommends cmake
```

- 没有新增工具 → 不构建，直接用；
- 有新增 → 只跑新增这一层，前面的安装不重复；
- 构建完更新标签里的包清单和 `created`，tag 不变，下次还能认出来。

Docker 自己的层缓存因此照常有用。`--no-accumulate` 才从原镜像重来，并且用新 tag，
不去动别人正在用的镜像。

### 3.6 记录在哪

镜像标签是权威来源：

```dockerfile
LABEL org.phprt.kind="toolchain" \
      org.phprt.base="docker.io/library/debian:13" \
      org.phprt.base.digest="sha256:9c6e…" \
      org.phprt.arch="amd64" \
      org.phprt.gcc="14.2" \
      org.phprt.pm="apt" \
      org.phprt.packages="gcc,g++,binutils,make,pkg-config,perl" \
      org.phprt.template="1" \
      org.phprt.created="2026-11-21T15:30:12Z"
```

本地再放一份索引 `.phprt/toolchains.json` 加速。索引丢了、或项目被搬走，就扫本地
`phprt-toolchain:*` 镜像的标签重建。项目 state 记住这次选中的 tag 和 base digest，
下次直接用。

### 3.7 Dockerfile 只用来看

每次真要构建时新生成一份，按 tag 分目录、时间戳命名，只增不删：

```text
.phprt/images/toolchain/
└── php-8.5-debian13/
    ├── 20261121T153012/Dockerfile     # 第一次：FROM debian13
    ├── 20261123T091500/Dockerfile     # 累加：FROM 上次的 toolchain
    └── 20261125T174401/Dockerfile     # --no-accumulate：FROM debian13
```

- 不解析旧文件、不拿它当缓存键、不接着上次的往下写；
- 复用判断只看镜像标签、索引、state；
- 构建失败也保留，方便看哪条 `RUN` 挂了，也方便用户自己 `podman build` 复现；
- 不需要构建时不生成；
- 结束时打印路径：

  ```text
  toolchain  phprt-toolchain:php-8.5-debian13（复用，补装 cmake）
  dockerfile .phprt/images/toolchain/php-8.5-debian13/20261123T091500/Dockerfile
  ```

删掉这些文件不影响任何复用——它们是记录，不是状态。

### 3.8 容器运行时

只做 podman、docker、kiri 三个。当前开发和验收只用 podman；docker 和 kiri 按同一套
参数实现，等有环境再补实测。kiri 是 Docker Engine API 兼容的运行时，`kiri` 命令接受
docker 写法，Linux 上 `kiri` 组等同 root。

只用四个命令：

```text
<bin> info                       判断能不能连
<bin> image inspect <镜像>       判断本地有没有、读标签
<bin> build -t <tag> -f <Dockerfile> <上下文>
<bin> run --rm ...               跑 plan / install
```

没有长驻容器、没有 `exec`、没有 `commit`。Dockerfile 是三家的共同语言，所以不需要
为每个运行时写不同的实现。

选择顺序：

1. `phprt.toml` 里 `container_runtime` 指定了 → 只用它，连不上就报错，不偷偷换；
2. 没写 → `podman` → `docker` → `kiri` 依次试 `info`；
3. 全失败 → 把每个候选的失败原因列清楚。

不接管 `DOCKER_HOST` / `docker context`；要固定 context 用户自己设。不 `sudo`。

### 3.9 容器里的用户

phprt 在容器里要写 state、日志、源码和产物，这些文件必须归宿主用户：

- rootful（docker、rootful podman）：`run --user <uid>:<gid>`；
- rootless（podman 默认、桌面版 Docker / Kiri）：不加 `--user`，容器里的 root 就是
  宿主用户。

判断方式靠实测，不按厂商猜：起一次 `--rm` 容器，在 `internal_dir` 写一个探针文件，
看属主是不是宿主 `geteuid()`，不是就加 `--user`。构建时设 `HOME=/tmp`。

SELinux enforcing 时挂载加 `:z`，不 enforcing 不加；不用 `:U`。

### 3.10 phprt 二进制进容器

把宿主当前的可执行文件挂到容器内固定路径，不往镜像里装 phprt。

限制：宿主二进制比原镜像的 glibc 新时，在里面跑会报 `GLIBC_2.xx not found`。第一版
直接报错并提示换同代镜像，不引入静态链接或容器内自举。phprt 只依赖 `libc` / `libm` /
`libgcc_s`，同代发行版可以直接拷进去跑。

### 3.11 签名、state、离线

- 构建签名加入原镜像的 **digest** 和「本地 / 镜像」两种方式。
- **工具链镜像 ID 不进签名**：累加工具不改变原镜像的 libc，不应该触发全量重编；
  换原镜像（digest 变）才重编。
- state 记录：原镜像 ref + digest、工具链 tag、状态文件路径。`--lock` 复用这些值。
- `--offline`：
  - 原镜像和工具链镜像都在本地 → 可以离线编译；
  - 要新建工具链镜像 → 不可能，直接报错；
  - 一次跑完且下载缓存为空 → 下载会失败，报错提示先联网准备。
- 镜像构建需要访问 apt / dnf 源，`bootstrap` 改源在这里生效。

### 3.12 二次跑的状态文件

`plan --buildbyimage --json` 输出的结构（也是 `--from-plan` 读的结构）：

```json
{
  "product": "php",
  "series": "8.5",
  "image": {
    "base": {
      "ref": "docker.io/library/debian:13",
      "digest": "sha256:9c6e…",
      "arch": "amd64"
    },
    "toolchain": {
      "tag": "phprt-toolchain:php-8.5-debian13",
      "from": "docker.io/library/debian:13@sha256:9c6e…",
      "exists": true
    }
  },
  "builder": {
    "tools": [
      {"name": "gcc", "need": ">=14", "found": "14.2", "status": "ok"},
      {"name": "patchelf", "need": ">=0.19", "found": null, "status": "source"},
      {"name": "perl", "need": ">=5.20", "found": "5.40", "status": "package",
       "packages": ["perl"], "check": "findbin"}
    ],
    "packages": {
      "pm": "apt",
      "install": ["gcc", "g++", "binutils", "make", "pkg-config"],
      "extra": ["perl"]
    },
    "bootstrap": []
  },
  "sources": []
}
```

`sources` 由宿主补：版本、URL、sha256、解压路径。文件落在
`.phprt/plan/<产品>-<系列>.json`。

### 3.13 plan 怎么跑

- 有可复用的工具链镜像 → 在它里面跑，看得到已经装了什么，只算增量；
- 没有 → 在原镜像里跑；
- `plan` 不弹选择：用 state 记录的；没有就用唯一兼容的；再没有就在原镜像里跑；
- 挂载项目根、`build/`（里面可能有上次从源码构建的 patchelf / autoconf）、recipes
  只读、phprt 二进制；
- plan 不下载、不写安装 state、不写 Dockerfile。

---

## 4. 数据

### 4.1 镜像 catalog

`recipes/images.toml`：

```toml
[images.debian13]
image  = "docker.io/library/debian:13"
digest = "sha256:9c6e3814…"
pm     = "apt"
gcc    = "14.2"
glibc  = "2.41"
packages = ["perl"]          # 追加到推导清单后面，写发行版真包名
# bootstrap = ["sed -i 's|deb.debian.org|mirrors.tuna…|g' /etc/apt/sources.list.d/debian.sources"]

[package_maps.apt]
gcc        = "gcc"
"g++"      = "g++"
binutils   = "binutils"
make       = "make"
cmake      = "cmake"
patchelf   = "patchelf"
perl       = "perl"
```

- `digest` 必填：tag 会漂，digest 才是身份。
- `gcc` 写 major.minor，用于构建前预检。
- `glibc` 只用于提示运行下限，不参与构建判断。
- `packages` / `bootstrap` 属于这条镜像的事实，且影响工具链镜像身份（改了要重建）。
- `[package_maps.<pm>]` 是命令名 → 包名，宿主安装提示和镜像装包共用一份数据。

### 4.2 bundle 默认镜像线

```toml
[image]
line = "debian13"
```

放在 `[image]` 表里，不占 bundle 顶层（顶层裸键留给有独立版本线的组件）。

### 4.3 state

```json
"image": {
  "line": "debian13",
  "image": "docker.io/library/debian:13",
  "digest": "sha256:...",
  "gcc": "14.2",
  "glibc": "2.41",
  "toolchain": "phprt-toolchain:php-8.5-debian13",
  "plan": ".phprt/plan/php-8.5.json"
}
```

可选字段：旧 state 没有时按 bundle 默认处理，schema 版本不升。

### 4.4 `phprt.toml`

```toml
container_runtime = "podman"   # 不写时自动探测 podman → docker → kiri
```

---

## 5. 代码改动点

| 文件 | 改动 |
| --- | --- |
| `src/container.rs` | 新增：运行时探测、原镜像 / 工具链镜像检查、Dockerfile 生成、`build` / `run`、属主探针、工具链索引读写 |
| `src/config.rs` | 解析 `container_runtime`；`--buildbyimage` 相关默认值 |
| `src/main.rs` | 新增 `--buildbyimage` / `--from-plan` / `--toolchain` / `--no-accumulate` |
| `src/plan.rs` / `src/render.rs` | 状态文件、`image` / `builder` 段、Dockerfile 预览 |
| `src/build.rs` | 签名加入原镜像 digest 与「本地 / 镜像」方式；容器内强制本地后端 |
| `src/state.rs` | 记录原镜像、工具链 tag、状态文件路径 |
| `recipes/images.toml` | 已存在；补齐容器实际用到的 `[package_maps.*]` 条目 |
| `crates/build/` | 不改：容器里的 phprt 就是本地执行 |

现有的求解、拓扑、签名、日志格式、日志路径都不动。如果实现中需要改 `plan.rs` 的
求解逻辑，说明接缝选错了，先回头改设计。

---

## 6. 测试与验收

### 6.1 纯函数

Dockerfile 生成、包清单合并、tag 计算（含 fresh hash）、兼容判断，表驱动断言，
不需要容器。

### 6.2 假运行时

PATH 上放一个记录 argv 的 `podman` 脚本，测：探测顺序、不自动 pull、失败文案、
`--no-accumulate` 产生新 tag、`container_runtime` 指定后不回落。

### 6.3 真机

当前只跑 podman；docker、kiri 按同一套参数实现，等有环境再补。

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets

# 原镜像必须本地已有
cargo run -- install php@8.5 --buildbyimage docker.io/library/debian:12   # 报错：先 pull
podman pull docker.io/library/debian:13

# 一次跑完
cargo run -- install php@8.5 --buildbyimage debian13
ls .phprt/images/toolchain/php-8.5-debian13/*/Dockerfile
.phprt/php-8.5.12/bin/php -v
.phprt/php-8.5.12/bin/php -m | grep -E 'openssl|curl|pdo_sqlite'

# 复用：不重装工具链
cargo run -- add php@8.5 swoole
cargo run -- install php@8.5
.phprt/php-8.5.12/bin/php -m | grep swoole

# 累加：新增工具才生成新 Dockerfile
cargo run -- install php@8.5 --component 'gd=feature:[jpeg]'
ls .phprt/images/toolchain/php-8.5-debian13/

# 二段跑
cargo run -- plan php@8.5 --buildbyimage --json > .phprt/plan/php-8.5.json
cargo run -- install php@8.5 --buildbyimage --from-plan .phprt/plan/php-8.5.json

# 不累加：新 tag，旧镜像不动
cargo run -- install php@8.5 --buildbyimage --no-accumulate

# 计划可校验
cargo run -- plan php@8.5 --buildbyimage --json | jq .builder

# 删掉 Dockerfile 不影响复用
rm -rf .phprt/images/toolchain/php-8.5-debian13
cargo run -- install php@8.5          # 仍应复用镜像，只编译
```

产物归属检查：

```bash
stat -c '%U:%G' .phprt/php-8.5.12/bin/php     # 必须是当前用户
```

### 6.4 落地顺序

每步独立验收：

1. **image catalog + 原镜像选择 + 记录**。不碰编译器。验收：`list_oci_images` 表格；
   三种 `--buildbyimage` 写法都能解析；不在本地时报错且不 pull；tag 和记录写对。
2. **plan 进容器 + 状态文件**。验收：空镜像里 plan 给出缺包清单；`--json` 能存成状态
   文件；工具分成四类。
3. **工具链镜像 + 复用累加**。验收：Dockerfile 生成并留档；镜像构建成功；第二次复用；
   只补差量；`--no-accumulate` 用新 tag；删 Dockerfile 不影响复用。
4. **容器内 install + 一次 / 二次跑**。验收：一次跑完能编出 PHP；`plan --json` +
   `--from-plan` 两段跑通；签名含原镜像 digest；`--offline` 可用。

第 1 步可以完全离线做，最容易先对齐语义。
