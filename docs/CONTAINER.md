# 容器构建：长驻容器

**本文是待实施的方案，代码尚未实现。** 镜像线（`recipes/images.toml`）和 OCI 产出
见 [IMAGE.md](IMAGE.md)。

容器模式只解决一件事：**别让用户每次编 PHP 都重装一遍 gcc**。做法是起一个不关的
容器，把装好的工具留在里面；`state.json` 记住这个容器，下次 `add` / `remove` /
`update` 直接 `podman exec` 进去用。

## 1. 模型

| 对象 | 谁产生 | 作用 |
| --- | --- | --- |
| 原镜像（base） | 用户自己 `podman pull` / `docker pull` | 只读底座；phprt 只检查本地有没有，不自动拉 |
| 容器实例 | phprt 创建，长期运行 | 装工具、编译；`state.json` 记它的名字和 id |

没有第三种东西：

- 不生成工具链镜像；
- 不写 Dockerfile，不写 compose；
- 不 `commit`、不 `export` / `import`；
- 不维护 `.phprt/toolchain-images.toml` 之类的镜像清单。

容器的文件系统在容器不在之后不复用。要重来就重新起一个容器、重装一遍工具；这就是
长驻容器存在的理由——不主动删就一直不用重来。

## 2. 容器实例

### 2.1 名字与创建

```text
phprt-<产品>-<系列>-<项目根 hash 8>
例：phprt-php-8.5-3f2a91c0
    phprt-php-8.4-77b0e5d2（同一个项目里的另一条系列）
```

带项目根 hash 是因为容器名在同一台机器上是全局的，两个项目不能撞名。用户不用记名字，
`state.json` 里有；提示删除时直接打印完整命令。

创建：

```bash
podman run -d --name phprt-php-8.5-3f2a91c0 \
  --workdir <项目根> \
  -v <项目根>:<项目根> \
  -v <internal_dir>:<internal_dir> \
  -v <download_dir>:<download_dir> \
  -v <recipes>:<recipes>:ro \
  -v <phprt 可执行文件>:/phprt:ro \
  <原镜像> sleep infinity
```

创建之后再以 root 跑镜像条目的 `bootstrap`（换源、`dnf makecache` 之类）：

```bash
podman exec -u 0 <容器> sh -c '<bootstrap 逐条>'
```

任何一条非 0 退出就删掉这个容器并报错，不留下一个装了一半的环境。

创建完成后先跑一次 `podman exec <容器> /phprt --version`：宿主二进制比原镜像的 glibc
新时这里就会报 `GLIBC_2.xx not found`，直接删掉容器并提示换同代镜像，不进入后面的
装包和编译。

`bootstrap` 只在创建容器时跑一次；`--offline` 不跑。宿主模式没有这一步。

### 2.2 挂载

```text
项目根                      读写（.phprt、build 默认都在里面）
internal_dir / build_dir    读写；配到项目根外时才单独挂
download_dir                读写
recipes                     只读
phprt 可执行文件            只读，固定挂在容器内 /phprt
```

路径保持宿主绝对路径，不翻译成容器路径：日志、state、错误信息在两边是同一份字符串。

容器创建后挂载不能再改。每次运行前算一遍本次需要的挂载集合，和 `state.json` 记的
不一致就重建容器。

### 2.3 容器里的用户

产物和 state 必须归宿主用户：

- rootless（podman 默认、桌面版 Docker / Kiri）：不加 `--user`，容器 root 就是宿主用户；
- rootful（docker、rootful podman）：创建时加 `--user <uid>:<gid>`。

判断不靠厂商猜：先看运行时自己的 rootless 标记，创建后写一个探针文件复核属主；
不一致就用另一种方式重建一次。

装包始终以 root 执行（`podman exec -u 0`），和容器主进程的用户无关。

### 2.4 用完不删

每次容器模式的命令结束都打印：

```text
容器 phprt-php-8.5-3f2a91c0 仍在运行（9b1d…），下次直接复用。
不再需要时删除：podman kill phprt-php-8.5-3f2a91c0 && podman rm phprt-php-8.5-3f2a91c0
```

phprt 自己不删容器。删了只影响速度：下次重新创建、重装工具，产物和 state 不受影响。

## 3. 每次运行先找容器

镜像模式的命令（`install` / `add` / `remove` / `update` / `build`，以及
`plan --buildbyimage`）都先读 `state.json` 的 `image.container`：

| 情况 | 行为 |
| --- | --- |
| 记录存在、容器在运行、原镜像 digest 和挂载一致 | 直接 `exec`，不重装 |
| 记录存在、容器已停止 | `start` 后继续 |
| 记录存在、容器已被 `rm` | 用同一个原镜像新建，提示“重建，工具会重装一遍” |
| 记录不存在（第一次） | 新建 |
| 原镜像 digest 变了 | 删掉旧容器（名字要复用），用新镜像新建 |
| 挂载集合变了 | 删掉旧容器，用同样的镜像新建 |
| 原镜像不在本地 | 报错，提示先 pull；不自动拉，也不新建 |

删旧容器只删 state 里记着的那一个，打印删了什么；别的项目的容器不动。

`plan --buildbyimage` 也要有容器才能探测工具：没有就创建，但只探测，不装包、不下载、
不编译。

判断只看三个命令：

```text
<bin> info                      能不能连
<bin> image inspect <镜像>      本地有没有、digest、架构
<bin> inspect <容器>            在不在、跑没跑、挂载和镜像
```

## 4. 流程

一次 `install` 是一条固定流水线，宿主和容器交替。每个检查点产出一份东西，
`--stop-after <检查点>` 在产出之后退出，把控制权交回宿主；不带参数就跑完。

### 4.1 检查点

| 检查点 | 在哪跑 | 产出 | 停在这里能做什么 |
| --- | --- | --- | --- |
| `packages` | 容器探测，宿主生成 | 装包命令（apt / dnf） | 换源、手工装包、改包名 |
| `mainline` | 容器 | 主线文件：依赖版本线、补丁、URL、sha256，工具来源 | 宿主按它做 catalog 更新 |
| `download` | 宿主 | 源码文件在 `download_dir` | 检查下载、搬缓存、离线准备 |
| `extract` | 宿主 | `build/` 下的源码目录 | 改上游源码、跑 `audit` |
| 不带 | 容器 | 安装树、state、入口 | — |

`packages` 的产出是一条可执行的命令，`mainline` 的产出是一个文件，`download` /
`extract` 的产出是磁盘上的东西。三种都不需要额外的服务或协议。

### 4.2 一次跑完

```bash
phprt install php@8.4 --buildbyimage
```

```text
宿主  1. image inspect；复用或创建容器；跑 bootstrap；验证 /phprt 能跑
容器  2. 求解依赖 + 探测工具：缺哪些包、哪些工具要自己编
宿主  3. 按 [package_maps.*] 生成装包命令，以 root 执行
容器  4. 复检工具，定下每个工具的来源（image / source），写主线
宿主  5. 按主线里的组件集合做 catalog 检查 / 更新
宿主  6. 下载（含 source 工具的源码）、解压
容器  7. 编译安装（source 工具在 build/tools 里编），写 state、入口
```

- 容器里的 phprt 认为自己在本地构建，`crates/build` 一行不用改。
- 容器内的调用由宿主发起，带内部环境变量 `PHPRT_IN_CONTAINER=1` 和检查点名；
  容器里的 phprt 只跑对应步骤，不再递归进容器。这是内部约定，不出现在 CLI 帮助里。
- 任何一步失败就停，已经装好的包、下载的文件、解压的目录都留着；重跑从失败处继续。

### 4.3 中断执行

默认不用停：`phprt install php@8.4 --buildbyimage` 会自己把包装好。装包也是 phprt
做的——宿主拼出 `podman exec -u 0 <容器> <pm> install …` 并执行，不需要用户复制命令。

想先看清单、或者要改命令时才停：

```bash
# ① 停在装包之前，看要装什么
phprt install php@8.4 --stop-after packages
#   需要安装：perl
#   podman exec -u 0 phprt-php-8.4-77b0e5d2 dnf install -y gcc gcc-c++ make perl

# ② 不打算改：直接重跑，phprt 自己执行装包
phprt install php@8.4

# ②' 想改（换源、加包）：自己执行改过的命令，再重跑
podman exec -u 0 phprt-php-8.4-77b0e5d2 dnf install -y gcc gcc-c++ make perl gdb
phprt install php@8.4
```

主线、下载、解压也是这样停：

```bash
phprt install php@8.4 --stop-after mainline
phprt install php@8.4 --stop-after download
phprt install php@8.4 --stop-after extract
phprt install php@8.4
```

- 每个 `--stop-after` 退出码 0，不写安装结果。
- 第一次 `--stop-after packages` 会先创建容器（否则没法探测），只创建、不装包。
- `--stop-after` 只决定在哪里退出，不跳过前面的步骤：不带检查点的命令会自己把
  包装好、把主线算出来。
- 再跑同一条命令（或不带参数）会重做前面的廉价检查：工具已装就跳过装包、
  文件在缓存就跳过下载、目录已解压就跳过、签名一致就跳过编译。

### 4.4 主线文件

`--stop-after mainline` 写 `.phprt/plan/<产品>-<系列>.json`，下次宿主侧的 phprt
直接读它：

```json
{
  "product": "php",
  "series": "8.4",
  "resolution_digest": "…",
  "image": {
    "ref": "quay.io/fedora/fedora:43",
    "digest": "sha256:40e0…",
    "container": "phprt-php-8.4-77b0e5d2"
  },
  "nodes": [
    {"id": "product:php@8.4.15", "name": "php", "line": "8.4", "version": "8.4.15",
     "url": "https://…/php-8.4.15.tar.xz", "sha256": "…"},
    {"id": "dep:openssl", "name": "openssl", "line": "3.5", "version": "3.5.9",
     "url": "https://…/openssl-3.5.9.tar.gz", "sha256": "…"},
    {"id": "tool:autoconf", "name": "autoconf", "version": "2.72", "provider": "source",
     "url": "https://…/autoconf-2.72.tar.xz", "sha256": "…"}
  ],
  "tools": [
    {"name": "gcc", "need": ">=14", "found": "15.3.1", "provider": "image"},
    {"name": "perl", "need": ">=5.20", "found": "5.40", "provider": "image",
     "packages": ["perl"]},
    {"name": "autoconf", "need": ">=2.69", "found": null, "provider": "source"}
  ]
}
```

- **写**：容器里的 phprt；**读**：宿主里的 phprt。
- `resolution_digest` 和当前方案对不上、或文件不存在 → 重新解析，不拿旧文件凑。
- `tools.provider` 只有两种：`image`（容器自带或装包满足）和 `source`（版本不满足，
  编译时在 `build/tools` 里自建）。
- 宿主做 catalog 更新时只看 `nodes` 里的组件，不扫全量；`source` 工具的源码也进
  下载清单。
- catalog 更新后补丁可能前进；下载和编译以更新后的 catalog 为准，主线提供的是
  组件集合和版本线。

### 4.5 为什么用命令行 + 文件，不用 gRPC

- 宿主和容器里跑的是同一个 phprt 二进制，没有第二套服务要部署；
- 每个检查点都是普通命令 + 一个文件，SSH、CI、`--offline` 都照常；
- 中断后靠幂等检查续跑，不依赖长连接或会话状态；
- 不引入端口、版本握手、鉴权。

以后要支持远程构建机（不在这台机器的 podman 上），再把这四个检查点换成 RPC，
语义不变。

`add` / `remove` / `update` / `build` 走同一条流水线，只是求解的输入不同：`remove`
少一个组件，`update` 前进补丁，`build` 编完再出 OCI 镜像。

## 5. 装包与复检

工具状态分四类：

| 状态 | 含义 | 后续 |
| --- | --- | --- |
| `ok` | 命令在、版本够、`feature_checks` 通过 | 不动，最终来源 `image` |
| `package` | 缺 / 版本不够 / 检查不过，但有包名映射 | 宿主 `exec` 装包，复检通过后变成 `image` |
| `source` | 没有可用包，但有 catalog 源码 | 最终来源 `source`，下载后在 `build/tools` 里构建 |
| `error` | 两样都没有 | 报错，点名去 `[package_maps.<pm>]` 补 |

`ok` / `package` / `source` / `error` 是探测时的判断；写进主线文件的 `tools.provider`
只有 `image` 和 `source` 两种最终来源。

- 包名来自 `[package_maps.<pm>]`，镜像条目的 `packages` 追加在后面（Debian /
  Ubuntu 的 `perl-base` 缺 `FindBin`，要装完整 `perl`）。
- 装包命令不带 `sudo`：容器里 `exec -u 0` 已经是 root。
- apt 先 `apt-get update`；dnf 直接 `install`。
- 装完复检一次（命令、版本、`feature_checks`）。仍缺就报错并列出缺什么，
  **不再装第二轮**，避免探测 → 装包 → 再装包的循环。
- 复检通过后，工具就留在容器里；下次 `add` / `update` 不再装。

## 6. 续跑与幂等

- 失败或打断都保留容器和已完成的部分；再跑同一条命令从第一个没完成的检查点继续。
- 每个检查点的判断都很便宜：容器 `inspect`、工具版本、主线 `resolution_digest`、
  文件在不在、`build` 签名。
- 装包只做一轮：复检仍缺就报错，不再装第二轮。
- 打断不写安装结果；容器记录在创建时就写进 state，所以下次不用再带 `--buildbyimage`。
- 换原镜像、换挂载、容器被删 → 容器重建；下载和解压的产物仍然复用。
- 装包默认由 phprt 执行；用户也可以自己执行，下一次运行看到的结果一样。

## 7. state 记录

```json
"image": {
  "line": "fedora43",
  "image": "quay.io/fedora/fedora:43",
  "digest": "sha256:40e0…",
  "gcc": "15.3",
  "glibc": "2.42",
  "container": {
    "name": "phprt-php-8.5-3f2a91c0",
    "id": "9b1d…",
    "created": "2026-11-22T00:45:00Z",
    "mounts": [
      "/home/z/blog:/home/z/blog",
      "/home/z/testphp/phprtv3/recipes:/home/z/testphp/phprtv3/recipes:ro"
    ]
  }
}
```

- 可选字段。旧 state 没有 `image` 时按宿主模式处理，schema 版本不升。
- 容器记录在容器创建后立即写进 state；只跑到检查点、安装失败或打断都保留，
  下一次才能接着用；`active` / `installed_version` 不受影响。
- 容器 id 只是缓存：id 对不上（容器被重建）就以 `inspect` 的结果为准，更新记录。
- 容器不参与方案指纹，也不参与节点签名。

## 8. 运行时

当前只做三个：

| 运行时 | 状态 |
| --- | --- |
| podman | 开发和验收都用它 |
| docker | 设计覆盖，未实测 |
| kiri | 设计覆盖，未实测；Docker Engine API 兼容，`kiri` 命令接受 docker 写法 |

只用这些命令：

```text
<bin> info
<bin> image inspect
<bin> inspect
<bin> run -d --name <容器> ...
<bin> exec [-u 0] <容器> ...
<bin> start / kill / rm
```

没有 `build`、`commit`、`export` / `import`。

选择顺序：

1. `phprt.toml` 的 `container_runtime` 指定了 → 只用它，连不上就报错，不回落；
2. 没写 → `podman` → `docker` → `kiri` 依次试 `<bin> info`；
3. 全失败 → 列清每个候选的失败原因。

不接管 `DOCKER_HOST` / `docker context`；不 `sudo`。

## 9. phprt 二进制进容器

挂宿主当前的可执行文件到 `/phprt`，不往镜像里装 phprt。

限制：宿主二进制比原镜像的 glibc 新时，在里面跑会报 `GLIBC_2.xx not found`。
第一版直接报错，提示换同代镜像；不做静态链接，也不在容器里重新编译 phprt。

## 10. 签名、离线

- 构建签名含原镜像 **digest** 和「本地 / 镜像」两种方式。
- 容器 id、容器创建时间不进签名：容器重建不该触发全量重编。换原镜像（digest 变）
  才重编。
- `--lock` 复用 state 里的镜像 digest 和容器名。
- `--offline`：
  - 容器在、工具已装好 → 可以离线编译；
  - 容器要新建、或工具要装包 → 报错，提示先联网准备；
  - 下载缓存为空 → 下载阶段报错，提示先联网准备。

## 11. 失败提示

原镜像不在本地：

```text
error: 本地没有 quay.io/fedora/fedora:43
phprt 不自动 pull。先执行：
  podman pull quay.io/fedora/fedora:43
```

工具既没有包映射也没有 catalog 源码：

```text
error: 工具 patchelf 在容器里不可用，也没有可用的包映射
在 recipes/images.toml 的 [package_maps.dnf] 里补 patchelf，或给 catalog 加一行
```

装完仍缺工具：

```text
error: 装完包复检仍缺：cmake
容器 phprt-php-8.5-3f2a91c0 保留着，可以手工进去看：
  podman exec -it phprt-php-8.5-3f2a91c0 sh
```

phprt 在容器里跑不起来：

```text
error: phprt 在 docker.io/library/debian:12 里跑不起来：GLIBC_2.38 not found
换 glibc 不低于宿主的原镜像。
```

## 12. 测试

1. **纯函数**：容器名、挂载集合、包清单合并、检查点边界、主线文件的 `resolution_digest`
   比较，表驱动断言，不需要容器。
2. **假运行时**：PATH 上放一个记录 argv 的 `podman` 脚本，测探测顺序、不自动 pull、
   容器不存在时重建、`container_runtime` 指定后不回落、失败文案。
3. **真机（podman）**：
   - 空镜像一次跑完，产出可用 PHP；
   - 第二次 `add swoole` 直接 `exec` 进同一个容器，不装 gcc；
   - `kill` / `rm` 容器后再跑，自动重建并提示重装；
   - `--stop-after mainline` 写出主线文件；手动作业后重跑能续上；
   - `--offline` 在容器就绪时可以编译，容器要重建时报错。

验收命令见 [IMAGE.md](IMAGE.md) 第 13 节。

## 13. 改动清单

| 文件 | 改动 |
| --- | --- |
| `src/container.rs` | 新增：运行时探测、容器检查 / 创建 / 启动、`exec`、挂载集合、属主探针 |
| `src/config.rs` | `container_runtime` 解析；`--buildbyimage` 默认值 |
| `src/main.rs` | `--buildbyimage` / `--stop-after`；检查点编排 |
| `src/plan.rs` / `src/render.rs` | `image` / `builder` 段；主线文件；检查点输出 |
| `src/build.rs` | 签名加原镜像 digest；容器内走本地后端 |
| `src/state.rs` | 记录原镜像、容器名 / id / 挂载 |
| `recipes/images.toml` | 已有；补容器实际用到的 `[package_maps.*]` |
| `crates/build/` | 不改：容器里的 phprt 就是本地执行 |

## 14. 不在范围

- 自动 pull 原镜像；
- 自动安装容器运行时、`sudo`；
- 生成工具链镜像、Dockerfile、compose；
- `commit` / `export` / `import`；
- 静态链接 phprt；
- Kubernetes / CRI、LXC / Incus、buildah、systemd-nspawn；
- 多架构构建。

## 15. 和其它文档的关系

- [IMAGE.md](IMAGE.md)：镜像线、原镜像 catalog、OCI 产出、验收命令。
- [BUILD.md](BUILD.md)：现有本地构建；容器里仍走同一套顺序和签名规则。
- [TOOLS.md](TOOLS.md)：工具检查、版本区间、`feature_checks`。
- [HOST-COMMANDS.md](HOST-COMMANDS.md)：宿主缺命令提示、包名映射。
- [STATE.md](STATE.md)：`image.container` 字段。
- [RPATH.md](RPATH.md)：失败要早报，对齐 patchelf 的前置检查。
