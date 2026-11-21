# 开发任务：OCI 容器构建（`--buildbyimage`）

本文是独立任务书：要做什么、用户怎么用、内部怎么实现。不依赖其它文档；读完这一份
就能开工。

状态：**待开发**。`recipes/images.toml` 已经有镜像条目，但目前只有宿主的安装提示在用它。
入口参数模型还没定，见第 7 节；第 2 节的 `--buildbyimage` 只是占位，不要按它实现。

---

## 1. 目的

在宿主上编译有两个问题：

1. **工具装一次就丢。** 换台机器、清一次环境，就要重装 gcc / make / perl / cmake；
   dnf / apt 很慢。
2. **宿主环境不可控。** 编译器版本、发行版、包名各不相同，同一份 recipe 在不同机器
   上结果不同。

做法：**从用户给的原镜像起一个不关的容器，工具装在容器里，编译也在容器里跑。**
宿主负责查 catalog、下载、解压。容器由 phprt 自己驱动，用户在容器里没有任何操作。

明确不做：

- 不生成工具链镜像，不写 Dockerfile，不写 compose，不 `commit` / `export` / `import`；
- 不自动 pull 原镜像；
- 不自动装容器运行时；
- 不静态链接 phprt；
- 不做跨架构构建，不推 registry；
- 这一版只支持 rootless 容器（见 §3.9）。

## 2. 用户使用方式

用户只跟宿主上的 phprt 打交道。容器里的命令是内部实现，不对外。

### 2.1 第一次：一条命令

```bash
podman pull quay.io/fedora/fedora:43      # 用户自己拉，phprt 不代劳
phprt install php@8.4 --buildbyimage
```

`--buildbyimage` 的值有三种写法：

| 写法 | 含义 |
| --- | --- |
| 不写值 | 用 bundle 里 `[image].line` 指定的默认镜像线 |
| 写线名（`fedora43`） | 用 `recipes/images.toml` 里 `[images.fedora43]` |
| 写完整镜像 ref | 用户自己准备的镜像；digest 现查，包管理器和工具版本在容器里现探 |

这条命令一直跑到编译成功或报错退出，不会中途停下来问什么；用户在容器里不需要做任何事。
（catalog 在线检查要不要询问，仍按现有配置。）

结束时打印：

```text
容器 phprt-php-8.4-77b0e5d2 仍在运行（9b1d…），下次直接复用。
不再需要时删除：podman kill phprt-php-8.4-77b0e5d2 && podman rm phprt-php-8.4-77b0e5d2
```

### 2.2 之后：不用再带参数

镜像模式和容器记录写在 state 里，后续命令沿用：

```bash
phprt add php@8.4 swoole     # 直接 exec 进同一个容器，不装 gcc
phprt install php@8.4
phprt remove php@8.4 xdebug
phprt update php@8.4
```

容器被 `kill` / `rm` 掉了，下次自动用同一个原镜像重建，并提示工具会重装一遍。

### 2.3 先看计划

想先知道容器里会装什么、编什么，用 `plan`；它只探测，不装包、不下载、不编译：

```bash
phprt plan php@8.4 --buildbyimage
```

输出包括原镜像、容器状态、每个工具的状态和要装的包，以及每个节点的版本线。

### 2.4 容器用完不删

每次容器模式的命令结束都打印还在运行和删除命令（见 §2.1）。phprt 自己不删容器；
删了只影响速度：下次重新创建、重装工具，产物和 state 不受影响。

### 2.5 报错

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
容器 phprt-php-8.4-77b0e5d2 保留着，可以手工进去看：
  podman exec -it phprt-php-8.4-77b0e5d2 sh
```

phprt 在容器里跑不起来：

```text
error: phprt 在 docker.io/library/debian:12 里跑不起来：GLIBC_2.38 not found
换 glibc 不低于宿主的原镜像。
```

rootful 运行时：

```text
error: 只支持 rootless 容器；当前 podman 是 rootful
用 rootless podman（普通用户直接运行）或 rootless docker
```

容器里的 phprt 没带内部标记：

```text
error: 检测到当前在容器里，但没有 --in-container
这个参数只由宿主传给容器里的 phprt，用户不需要手动使用。
```

## 3. 原理

### 3.1 谁在哪跑

| 角色 | 跑什么 |
| --- | --- |
| 宿主上的 phprt | 建容器、`podman exec`、查 catalog、下载、解压、写最终 state |
| 容器里的 phprt | 求解依赖、探测和装工具、输出包列表、编译安装 |

宿主和容器里是同一个二进制。容器里的进程由宿主启动，一律带内部参数
`--in-container`：它告诉 phprt 只做容器本地的事，不再建容器、不再 `podman exec`。
这个参数不出现在 `--help` 里，用户不需要、也不应该手动用。

容器环境由 `/.dockerenv` / `/run/.containerenv` 识别：在容器里却没带
`--in-container` 时直接报错，避免容器里的 phprt 又去建容器。

### 3.2 宿主怎么驱动容器

一次 `install` 分三段，全部由宿主发起：

```text
1. 确保容器存在（建或复用）
2. 跑容器第一阶段：解析依赖、装工具、输出包列表
3. 宿主自己：按包列表查 catalog 补丁、下载、解压、算出精确版本表
4. 跑容器第二阶段：按精确版本表编译安装，写 state
```

对应的命令：

```text
podman exec <容器> phprt install php@8.4 --in-container
     → stdout 是包列表（表 A）

宿主进程内：catalog 更新 + 下载 + 解压
     → 表 B

podman exec -i <容器> phprt install php@8.4 --in-container
     ← stdin 是表 B
```

两段的区别只看 stdin：

- 没有表（`podman exec` 不带 `-i`）→ 做第一阶段，打印表 A，退出 0；
- 有表（`podman exec -i` + 表 B）→ 做第二阶段，编译安装，写 state。

宿主控制 `-i`，所以这个判断是确定的。宿主还会校验：第二阶段如果没收到表（stdin 为空）
或者输出了表格式的内容，按错误处理，不当成安装成功。

### 3.3 表格式

一行一条记录，三个字段，空格分隔：

```text
<kind> <name> <ref>
```

- `kind`：`product` / `dep` / `component` / `tool`；
- `name`：catalog 里的名字；
- `ref`：表 A 里是版本线（`3.5`），`tool` 是具体版本（`2.72`）；
  表 B 里全是精确补丁（`3.5.9`）。
- 空行和 `#` 开头的行忽略；格式不对直接报错。

为什么加 `kind`：PHP 的 bundled 组件 `openssl` 和依赖库 `openssl` 同名，只有名字分不开。

只列需要单独下载源码的节点：

- `product`：产品源码；
- `dep`：有独立源码的依赖库；
- `component`：外部组件（PECL、独立仓库）；bundled 组件不列，它的源码在产品包里；
- `tool`：宿主没有、要自己编的构建工具。

表 A 例：

```text
product php 8.4
dep openssl 3.5
dep libzip 2.1
component swoole 6.2
tool autoconf 2.72
```

表 B 例：

```text
product php 8.4.15
dep openssl 3.5.9
dep libzip 2.1.5
component swoole 6.2.5
tool autoconf 2.72
```

stdout 只放协议行；装包、下载、catalog 的提示全部走 stderr。

### 3.4 第一阶段：解析、装工具、出表

1. 按 argv、bundle、state 求解依赖图，得到版本线；
2. 探测工具：命令在不在、版本够不够 `tool.min`、`feature_checks` 过不过；
3. 缺的按 `[package_maps.<pm>]` + `[images.*].packages` 装包；
4. 装完复检一次，仍缺就报错，不再装第二轮；
5. 打印表 A，退出 0。

不写安装结果（容器记录创建时就写了）、不查 catalog、不下载、不编译。

工具装包直接发生在容器里，以 root 跑容器自己的包管理器：

- 命令名来自产品 `toolchain` 的 gcc / g++，以及所有节点 `tools` 的声明；
- 四类结果：

  | 状态 | 含义 | 处理 |
  | --- | --- | --- |
  | `ok` | 在、版本够、检查通过 | 不动 |
  | `package` | 缺 / 版本不够 / 检查不过，但有包名映射 | 装包 |
  | `source` | 没有可用包，但有 catalog 源码 | 进宿主下载清单，第二阶段在 `build/tools` 构建 |
  | `error` | 两样都没有 | 报错，点名补 `[package_maps.<pm>]` |

- apt 先 `apt-get update`；dnf 直接 `install`；不写 `sudo`。

典型坑：Debian / Ubuntu 的 `perl-base` 有 `/usr/bin/perl` 但缺 `FindBin`，openssl 的
`./Configure` 会失败。这类“命令在但检查不过”按 `package` 处理，装完整 `perl`。

### 3.5 中间：宿主准备源码

1. 读表 A；
2. 只对表里的组件查 catalog 最新补丁，不扫全量；补丁可能前进；
3. 下载产品、依赖、外部组件和 `source` 工具的源码到 `download_dir`；
4. 解压到 `build/`；
5. 得到表 B。

这一步不碰容器，容器不在也能做；它只用 recipes、catalog 和宿主缓存。
工具（第一阶段的装包）用的是容器里的包管理器，和这一步无关。

### 3.6 第二阶段：按表编译

1. 读表 B，把每个版本当精确补丁（等同 `--lock`）；
2. 直接从 `build/` 里已解压的源码 configure / make / install，不再查 catalog、不再下载；
3. 写 state、入口、完成标记。

工具在这一步之前已经装好。容器被重建、工具丢了时，按第一阶段重新装一遍；不在这里
偷偷装。

### 3.7 容器

名字：

```text
phprt-<产品>-<系列>-<项目根 hash 8>
例：phprt-php-8.4-77b0e5d2
```

容器名在同一台机器上是全局的，带项目根 hash 才不会和别的项目撞名。

创建：

```bash
podman run -d --name phprt-php-8.4-77b0e5d2 \
  --workdir <项目根> \
  -v <项目根>:<项目根> \
  -v <internal_dir>:<internal_dir> \
  -v <download_dir>:<download_dir> \
  -v <build_dir>:<build_dir> \
  -v <recipes>:<recipes>:ro \
  -v <phprt 可执行文件>:/usr/local/bin/phprt:ro \
  <原镜像> sleep infinity
```

- 不设环境变量；是不是在容器里只由 `--in-container` 决定。
- 挂载在项目根里的目录不重复挂；路径保持宿主绝对路径，不翻译。
- 创建后以 root 跑镜像条目的 `bootstrap`（换源、`dnf makecache` 之类），非 0 退出
  就删掉容器并报错；`--offline` 不跑。
- 创建后跑一次 `phprt --version`，确认宿主二进制在镜像里能跑。

每次运行先查 state 里的容器记录：

| 情况 | 行为 |
| --- | --- |
| 在运行，原镜像 digest 和挂载一致 | 直接用 |
| 已停止 | `start` 后继续 |
| 已被 `rm` | 用同一个原镜像重建，提示工具会重装 |
| 第一次 | 创建 |
| 原镜像 digest 变了 | 删掉旧容器，用新镜像创建同名容器 |
| 挂载集合变了 | 删掉旧容器，用同样的镜像创建 |

只用这些命令：`info` / `image inspect` / `inspect` / `run -d` / `exec` / `start` /
`kill` / `rm`。不 `build`、不 `commit`。

### 3.8 state

state 文件在 `.phprt/<产品>-<系列>/state.json`；镜像模式多一个 `image` 段：

```json
"image": {
  "line": "fedora43",
  "image": "quay.io/fedora/fedora:43",
  "digest": "sha256:40e0…",
  "gcc": "15.3",
  "glibc": "2.42",
  "container": {
    "name": "phprt-php-8.4-77b0e5d2",
    "id": "9b1d…",
    "created": "2026-11-22T00:45:00Z",
    "mounts": ["/home/z/blog:/home/z/blog"]
  }
}
```

- 可选字段。旧 state 没有 `image` 时按宿主模式处理，schema 版本不升。
- 容器记录在创建后立即写；只跑到第一阶段或中途失败也保留，下一次才能接着用；
  `active` / `installed_version` 不受影响，安装是否完成仍看完成标记。
- 容器 id 只是缓存；和 `inspect` 对不上就以 `inspect` 为准。
- 容器不参与方案指纹，也不参与节点签名。构建签名含原镜像 **digest** 和「本地 / 镜像」
  方式；换原镜像才重编。

### 3.9 幂等、失败、离线

- 两段都幂等：工具已装跳过、下载命中缓存跳过、目录已解压跳过、签名一致跳过编译。
  整条命令重跑安全。
- 任一段非 0 退出就停；容器、已下载的文件、已解压的目录都保留。
- 第一段不写安装结果；第二段成功才写。
- `--offline`：容器在、工具已装、源码已下载 → 可以离线编译；要新建容器、要装包、
  缓存为空 → 报错，提示先联网准备。

### 3.10 运行时与 rootless

当前只做三个运行时：

| 运行时 | 状态 |
| --- | --- |
| podman | 开发和验收都用它 |
| docker | 设计覆盖，未实测 |
| kiri | 设计覆盖，未实测；Docker Engine API 兼容，`kiri` 命令接受 docker 写法 |

选择顺序：

1. `phprt.toml` 的 `container_runtime` 指定了 → 只用它，连不上就报错，不回落；
2. 没写 → `podman` → `docker` → `kiri` 依次试 `info`；
3. 全失败 → 列清每个候选的失败原因。

容器里以 root 跑 phprt（装包需要）。这一版只支持 rootless：容器里的 root 映射到宿主
用户，产物和 state 属主天然正确。`info` 显示 rootful 时直接报错，不做 `--user` + chown。

## 4. 为什么用 stdio，不用 gRPC

- 宿主和容器里是同一个二进制，没有第二套服务要部署；
- 两段之间就是一张表，SSH、CI、`--offline` 都照常；
- 管道就是协议，断开、日志、退出码都是 shell 现成的；
- 不引入端口、版本握手、鉴权。

以后要接远程构建机，把两段换成 RPC 即可，表的结构不变。

## 5. 代码改动点

| 文件 | 改动 |
| --- | --- |
| `src/container.rs` | 新增：运行时探测、rootless 判断、容器检查 / 创建 / 启动、`exec`、挂载集合 |
| `src/config.rs` | 解析 `container_runtime`；`--buildbyimage` 默认值 |
| `src/main.rs` | `--buildbyimage`（公开）；`--in-container`（内部，隐藏）；宿主驱动两段 |
| `src/plan.rs` | 输出表 A；读表 B 作为精确补丁；表格式解析 |
| `src/render.rs` | `plan --buildbyimage` 的 `image` / `builder` 段 |
| `src/build.rs` | 签名加原镜像 digest；容器内走本地后端 |
| `src/state.rs` | 记录原镜像和容器 |
| `recipes/images.toml` | 补容器实际用到的 `[package_maps.*]` |
| `crates/build/` | 不改：容器里的 phprt 就是本地执行 |

现有的求解、拓扑、签名、日志格式不动。

## 6. 验收

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets
```

纯函数（不需要容器）：容器名、挂载集合、包清单合并、表格式的解析和输出、
rootless 判断，表驱动断言。

假运行时：PATH 上放一个记录 argv 的 `podman` 脚本，测探测顺序、不自动 pull、
容器不存在时重建、`container_runtime` 指定后不回落、rootful 报错、
容器内调用带 `--in-container`。

真机（当前只跑 rootless podman）：

```bash
# 原镜像必须本地已有
phprt install php@8.4 --buildbyimage docker.io/library/debian:12   # 报错：先 pull
podman pull quay.io/fedora/fedora:43

# 一条命令
phprt install php@8.4 --buildbyimage fedora43
.phprt/php-8.4.*/bin/php -v
.phprt/php-8.4.*/bin/php -m | grep -E 'openssl|curl|pdo_sqlite'

# 先看计划
phprt plan php@8.4 --buildbyimage

# 复用：不重装工具
phprt add php@8.4 swoole
phprt install php@8.4
.phprt/php-8.4.*/bin/php -m | grep swoole

# 容器没了自动重建
podman rm -f phprt-php-8.4-77b0e5d2
phprt install php@8.4

# 产物属主
stat -c '%U:%G' .phprt/php-8.4.*/bin/php     # 必须是当前用户

# 内部参数不出现在帮助里
phprt install --help | grep -c in-container   # 输出 0
```

落地顺序：

1. image catalog + 原镜像选择 + 容器创建 / 复用 / 删除提示；
2. 容器里求解、探测、装包、输出表 A；
3. 宿主读表、catalog 更新、下载、解压、输出表 B；
4. 容器里读表、精确版本编译、写 state；
5. `build` 复用同一套。

## 7. 未决：容器化的入口

**这一节是下次要先解决的问题。本文其它章节的 `--buildbyimage` 是占位，不要按它实现。**

当前写法把 `--buildbyimage` 挂在 `install` / `add` / `remove` / `update` 上。这不合理：

- 容器化是一次选择，不是每条命令都要重复的参数；
- 用户应该在动手之前先确认“之后的 install / add / remove / update 都在容器里编译”，
  确认之后就不再带参数；
- 同一份项目里，容器模式应该只有一个来源，不应该每条命令各自决定。

镜像也不应该由用户在命令行里指定：

- 产品系列的 `toolchain` 声明了 gcc / gxx 的档位，这就是构建条件；
- 镜像应该由这套声明从 `recipes/images.toml` 里自动选出；
- 用户只负责“选中的镜像在本地”，phprt 不自动 pull；
- 换个镜像应该是改 recipe / bundle / catalog，而不是在命令上加参数。

要定下来的问题：

1. **开关的形态**：项目级还是产品级；用 `phprt container use` 这种子命令，还是并进
   现有的初始化 / 配置流程；开启时要不要交互确认；怎么关闭、怎么查看。
2. **镜像选择规则**：`[image].line` 和“按 gcc 档位筛”是什么关系；多条候选时用哪条；
   没有 `toolchain` 的产品（如 nginx）怎么选；候选为空时报什么错。
3. **开关记住之后**：`install` / `add` / `remove` / `update` / `plan` / `scheme`
   / `build` 分别怎么读它；还需不需要一个一次性的“回到宿主”入口。
4. **参数面**：`--buildbyimage` 是删掉，还是留作只给调试用的隐藏参数。

在这些定下来之前，第 2 节的用法、第 5 节的 `src/main.rs` 改动、第 6 节的验收命令
都只是占位，不能直接开工。

