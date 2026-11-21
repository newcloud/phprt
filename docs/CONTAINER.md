# 容器构建与工具链镜像

**本文是待实施的方案，代码尚未实现。** `--buildbyimage` 落地时按本文实现；镜像线
（`recipes/images.toml`）、builder 装包、OCI 输出见 [IMAGE.md](IMAGE.md)。

本文回答三件事：工具链镜像怎么来、怎么复用、phprt 在容器里怎么跑。

## 1. 两种镜像，一个 Dockerfile

| 对象 | 谁产生 | 作用 |
| --- | --- | --- |
| 原镜像（base） | 用户自己 `podman pull` / `docker pull` | 只读底座；phprt 只检查本地有没有，不自动拉 |
| 工具链镜像（toolchain） | phprt 生成 | 带 gcc / make / perl 等构建工具，可复用、可累加 |
| Dockerfile | phprt 每次生成 | 给人看的记录；phprt 不复用它，也不解析它 |

原镜像是 `debian13`、`fedora43` 这种镜像线，也可以是用户自己 pull 的任意
`镜像@digest`。工具链镜像默认名：

```text
phprt-toolchain:<产品>-<系列>-<原镜像标识>
例：phprt-toolchain:php-8.5-debian13
    phprt-toolchain:php-8.5-custom-9c6e38a1
```

`<原镜像标识>` 是 catalog 线名；用户直接给镜像 ref 时用 digest 前 8 位。这样换一个
原镜像就会用另一个 tag，不会把旧的工具链覆盖掉。

## 2. 一次跑完

默认方式，一条命令把 plan、建镜像、下载、解压、编译全部串起来：

```text
phprt install php@8.5 --buildbyimage
```

```text
宿主
 1. image inspect 原镜像；本地没有就报错，不自动 pull
 2. 起容器跑 phprt plan
      镜像 = 已有可复用的工具链镜像；没有就是原镜像
      挂载 = 项目根、build/、recipes 只读、phprt 二进制
      → 工具状态 + 包清单 + Dockerfile 内容
 3. 需要装新工具时：写 Dockerfile，podman/docker/kiri build
 4. 起工具链镜像的容器跑 phprt install
      容器里就是本地模式：catalog 检查、下载、解压、编译、安装、写 state
 5. 退出码、日志、state 都通过挂载回到宿主
```

关键点：

- **phprt 本身进容器跑**，不是宿主逐条 `exec`。`crates/build` 一行不用改，容器里
  的 phprt 认为自己在本地构建。
- 宿主只做四件事：检查镜像、构建镜像、起容器、传挂载。不关心产品是 PHP 还是 nginx。
- 下载和解压都在容器里做，因为这是一次跑完；`download_dir` 和项目根挂进去，
  宿主已有的缓存直接复用，`--offline` 也能用。
- 容器里禁止再次进入镜像模式（否则递归）。用 `--in-container` 或环境变量
  `PHPRT_IN_CONTAINER=1` 标记，内部按本地后端跑。

挂载清单：

```text
项目根                        读写（.phprt、build 默认都在里面）
internal_dir / build_dir      读写（配到项目根外时补挂）
download_dir                  读写（一次跑完时容器要下载）
recipes                       只读
phprt 二进制                  只读，挂到容器内固定路径（如 /phprt）
```

路径保持宿主绝对路径不变，不做「宿主路径 → 容器路径」翻译。

## 3. 二次跑

想先看状态、或者下载必须走宿主网络时，把流程停成两段：

```bash
# ① 容器里只跑到工具状态，不下载、不编译；标准输出就是状态文件
phprt plan php@8.5 --buildbyimage --json > .phprt/plan/php-8.5.json

# ② 宿主补全状态：catalog 查版本、下载、解压
#    然后进工具链镜像只做编译
phprt install php@8.5 --buildbyimage --from-plan .phprt/plan/php-8.5.json
```

- `plan --buildbyimage --json` 只跑解析和工具判断，退出码 0，不写安装 state；
  输出里的 `image` / `builder` 段就是宿主后续要用的状态。
- `--from-plan` 读状态文件，宿主完成 catalog / 下载 / 解压，确保工具链镜像
  （没有就按状态里的 Dockerfile 内容构建），再起容器编译；容器里不重新解析，
  也不再下载。
- 一次跑完也写这个状态文件，只是不停下来，方便事后核对。

状态文件内容（宿主可读、可改）：

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

`sources` 由宿主补：版本、URL、sha256、解压路径。容器编译时只读这些，不再查
catalog。

## 4. 工具链镜像的复用与累加

装工具链很慢，所以第二次用同一个原镜像时，默认复用上次的镜像，只补新增工具。

### 4.1 记录

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

本地再放一份索引加速，丢了对得上：

```text
.phprt/toolchains.json
```

索引没有、或项目被搬走时，扫本地 `phprt-toolchain:*` 镜像的标签补回来。

项目 state 里记住这次选中的 toolchain（tag + base digest），下次 `install` / `add` /
`remove` / `update` 不再问，直接复用。`--toolchain` 和 `--no-accumulate` 选了新镜像时
同步更新这条记录。

### 4.2 选择顺序

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

1. `--toolchain <tag>` 指定了 → 用它；
2. 项目 state 里记过、镜像还在、仍兼容 → 直接用，不问；
3. 只有一条兼容 → 自动复用，打一行提示；
4. 多条兼容 → 列出来让用户选；
5. 没有兼容的 → 从原镜像新建，不弹选择。

非交互（没有 TTY、CI）：能自动选就选，选不出来就报错并列出候选，不阻塞。

兼容条件：

- 原镜像 digest 和架构相同（同 tag 但 digest 变了，算不同系统）；
- gcc 版本落在当前产品的 `toolchain` 区间；
- 镜像还在本地；
- 缺的工具能继续累加。

不满足的镜像也列出来，但标「不兼容」，不让选。

`--buildbyimage` 给完整镜像 ref 时规则一样：digest 对得上已有工具链就复用，
对不上就新建；`--no-accumulate` 直接跳过复用。

### 4.3 累加怎么构建

选中旧镜像后，新的 Dockerfile 从它接着装：

```dockerfile
FROM phprt-toolchain:php-8.5-debian13
RUN apt-get update && apt-get install -y --no-install-recommends cmake
```

- 没有新增工具 → 不构建，直接用；
- 有新增 → 只跑新增这一层，前面的安装不重复；
- 构建完更新标签里的包清单、`created` 和索引；
- tag 不变，下次还能认出来。

`--no-accumulate` 不从旧镜像接，直接从原镜像装。为了不破坏别的项目正在用的镜像，
它用一个新 tag：

```text
phprt-toolchain:<产品>-<系列>-<原镜像标识>-fresh-<包清单 + bootstrap 的 hash 8>
```

同样的包清单会得到同样的 fresh tag，可以复用。旧镜像不动；选中后 state 改记这个
新 tag，后续沿用。

`add swoole` 这种后续操作走同一套：

```text
phprt add php@8.5 swoole
  → 在已记录的工具链镜像里跑 plan
  → 工具没变化 → 不构建，直接复用
  → 只编 swoole，装完写 state
```

### 4.4 换原镜像

原镜像 digest 变了，旧工具链镜像不再兼容：

- 默认 tag 相同（比如都叫 `…-debian13`），从新 digest 重新构建，覆盖这个 tag；
- 旧镜像的层还在，直到运行时自己回收；
- 换的是另一条镜像线（debian13 → fedora43）时 tag 本身就不同，互不影响。

## 5. Dockerfile：留着，但不复用

每次需要构建工具链时新生成一份，按 tag 分目录、时间戳命名，只增不删：

```text
.phprt/images/toolchain/
└── php-8.5-debian13/
    ├── 20261121T153012/Dockerfile     # 第一次：FROM debian13
    ├── 20261123T091500/Dockerfile     # 累加：FROM 上次的 toolchain
    └── 20261125T174401/Dockerfile     # --no-accumulate：FROM debian13
```

- phprt 不解析旧 Dockerfile，不拿它当缓存键，也不接着上次的文件写；
- 下次构建重新生成一个新目录；
- 复用判断只看镜像标签、索引、state；
- Docker 自己的层缓存照常生效，`FROM 旧镜像 + 新增包` 不会全量重装；
- 构建失败也保留，方便看是哪条 `RUN` 挂了，也方便用户自己 `podman build` 复现；
- 不需要构建时不生成（复用了镜像、又没有新增工具）；
- `install` / `build` 结束时打印路径：

  ```text
  toolchain  phprt-toolchain:php-8.5-debian13（复用，补装 cmake）
  dockerfile .phprt/images/toolchain/php-8.5-debian13/20261123T091500/Dockerfile
  ```

`plan` 只预览 Dockerfile 内容，不写文件；写文件发生在需要构建工具链的时候。

## 6. plan 跑在哪

- 有可复用的工具链镜像 → 在它里面跑，看得到已经装了什么，只算增量；
- 没有 → 在原镜像里跑。
- `plan` 不弹选择：用 state 记录的；没有就用唯一兼容的；再没有就在原镜像里跑。

挂载：项目根、`build/`（里面可能有上次从源码构建的 patchelf / autoconf）、
recipes 只读、phprt 二进制。plan 不下载、不写安装 state。

plan 输出里直接带一条可放进 Dockerfile 的安装命令：

```text
builder  phprt-toolchain:php-8.5-debian13（复用）
install  apt-get update && apt-get install -y --no-install-recommends cmake
```

plan 对每个工具给出四种结果：

| 结果 | 含义 | 后续 |
| --- | --- | --- |
| `ok` | 命令在、版本满足、`feature_check` 通过 | 不动 |
| `package` | 缺命令、版本不够或检查不过，但有包名映射 | 写进 Dockerfile 装包 |
| `source` | 有 catalog recipe，但没有可用包 | 宿主下载，编译时在 `build/tools` 里构建 |
| `error` | 既没有包映射，也没有 catalog recipe | 报错，点名去 `[package_maps.<pm>]` 补 |

- 命令在但 `feature_check` 不过（Debian 的 `perl-base` 缺 `FindBin`）按 `package`
  处理，装镜像条目的 `packages` 或映射到的完整包。
- 包版本不够、catalog 也没有满足的线 → `error`，不自动降级。
- 工具链镜像构建完，再在镜像里跑一次版本和 `feature_check`，防止包源给错版本。

## 7. 容器运行时

当前只做三个：

| 运行时 | 状态 |
| --- | --- |
| podman | 开发和验收都用它；Fedora 43 rootless 已跑通手工流程 |
| docker | 设计覆盖，未实测 |
| kiri | 设计覆盖，未实测；Docker Engine API 兼容，`kiri` 命令接受 docker 写法 |

探测用一个命令，工作用三个：

```text
<bin> info                       判断能不能连
<bin> image inspect <镜像>       判断本地有没有、读标签
<bin> build -t <tag> -f <Dockerfile> <上下文>
<bin> run --rm ...               跑 plan / install
```

没有长驻容器、没有 `exec`、没有 `commit`。Dockerfile 是三家共同语言。

选择顺序：

1. `phprt.toml` 的 `container_runtime` 指定了 → 只用它，连不上就报错，不回落；
2. 没写 → `podman` → `docker` → `kiri` 依次试 `<bin> info`；
3. 全失败 → 列清每个候选的失败原因。

不接管 `DOCKER_HOST` / `docker context`；要固定 context 用户自己设。不 `sudo`。

## 8. 运行容器时的用户

phprt 在容器里会写 state、日志、源码和产物，这些文件必须归宿主用户，不能变成 root。

- rootful（docker、rootful podman）：`run --user <uid>:<gid>`；
- rootless（podman 默认、桌面版 Docker / Kiri）：不加 `--user`，容器 root 就是宿主用户。

判断方式仍然是实测，不按厂商猜：起一次 `--rm` 容器，在 `internal_dir` 写一个探针文件，
看属主是不是宿主 `geteuid()`；不是就加 `--user`。构建时设 `HOME=/tmp`。

SELinux enforcing 时挂载加 `:z`；不 enforcing 不加。不使用 `:U`。

## 9. phprt 二进制进容器

挂宿主当前的可执行文件到容器内固定路径，不往镜像里装 phprt。

限制：宿主二进制比原镜像的 glibc 新时，在里面跑会报 `GLIBC_2.xx not found`。
第一版直接报错，提示换同代镜像；不引入静态链接或容器内自举。

## 10. 签名、state、离线

- 构建签名含 **原镜像 digest** 和「本地 / 镜像」两种方式，不含工具链镜像 ID。
  累加工具不触发全量重编；换原镜像才重编。
- state 记录：原镜像 ref + digest、工具链 tag、状态文件路径；`--lock` 复用这些值。
- `--offline`：
  - 原镜像和工具链镜像都在本地 → 可以离线编译；
  - 要新建工具链镜像 → 不可能，直接报错；
  - 一次跑完且缓存为空 → 下载会失败，报错提示先联网准备。
- 镜像构建需要访问 apt/dnf 源；`bootstrap` 改源在这里生效。

## 11. 失败提示

原镜像不在本地：

```text
error: 本地没有 docker.io/library/debian:13@sha256:9c6e…
phprt 不自动 pull。先执行：
  podman pull docker.io/library/debian:13
```

工具既没有包映射也没有 recipe：

```text
error: 工具 patchelf 在工具链镜像里不可用，也没有可用的包映射
在 recipes/images.toml 的 [package_maps.apt] 里补 patchelf，或给 catalog 加一行
```

工具链镜像不兼容：

```text
error: phprt-toolchain:php-8.5-debian13 基于 debian:13@sha256:aaaa…，
当前原镜像是 sha256:bbbb…；不能累加。
加 --no-accumulate 从原镜像新建，或换回原来的原镜像。
```

phprt 在容器里跑不起来：

```text
error: phprt 在 docker.io/library/debian:12 里跑不起来：GLIBC_2.38 not found
换 glibc 不低于宿主的原镜像，或等静态链接版本。
```

## 12. 测试

1. **纯函数**：Dockerfile 生成、包清单合并、tag 计算、兼容判断，表驱动断言，
   不需要容器。
2. **假运行时**：PATH 上放记录 argv 的 `podman` 脚本，测探测顺序、不自动 pull、
   失败文案、`--no-accumulate` 的 tag。
3. **真机**：当前只跑 podman；docker、kiri 按同一套参数实现，等有环境再补：
   - 空镜像一次跑完，产出可用 PHP；
   - 第二次 `add swoole` 复用工具链，只编译新增部分；
   - `plan --json` + `--from-plan` 两段跑通；
   - 删除 Dockerfile 不影响任何复用（证明它只是记录）。
4. **Dockerfile**：同一 tag 下多次累加，目录按时间戳递增，不覆盖；失败也保留。

## 13. 实施时的改动清单

| 文件 | 改动 |
| --- | --- |
| `src/container.rs` | 新增：运行时探测、base/toolchain 检查、Dockerfile 生成、build/run、属主探针 |
| `src/config.rs` | `container_runtime` 解析；`--buildbyimage` 相关默认值 |
| `src/main.rs` | `--buildbyimage` / `--from-plan` / `--toolchain` / `--no-accumulate` |
| `src/plan.rs` / `src/render.rs` | 状态文件、`builder` 段、Dockerfile 预览 |
| `src/build.rs` | 签名加原镜像 digest；容器内走本地后端 |
| `src/state.rs` | 记录原镜像、工具链 tag、状态文件 |
| `crates/build/` | 不改：容器里的 phprt 就是本地执行 |
| `phprt.toml` | 注释里补 `container_runtime` |
| 文档 | 本文；IMAGE.md §4 / §7 / §12；CLI.md 的 `--buildbyimage`；BUILD.md 的「构建环境」 |

## 14. 不在范围

- 自动 pull 原镜像；
- 自动安装容器运行时、自动 `sudo`；
- 长驻容器 + `exec`；
- 静态链接 phprt；
- Kubernetes / CRI、LXC / Incus、buildah、systemd-nspawn；
- `docker build` / Dockerfile 生成之外的镜像构建方式；
- 多架构构建。

## 15. 和其它文档的关系

- [IMAGE.md](IMAGE.md)：镜像线、原镜像 catalog、OCI 输出。
- [BUILD.md](BUILD.md)：现有本地构建；容器里仍走同一套顺序和签名规则。
- [TOOLS.md](TOOLS.md)：工具检查、版本区间、`feature_checks`。
- [HOST-COMMANDS.md](HOST-COMMANDS.md)：宿主缺命令提示；容器模式的包清单位于
  状态文件里。
- [RPATH.md](RPATH.md)：失败要早报，对齐 patchelf 的前置检查。
