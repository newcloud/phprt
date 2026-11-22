# OCI 构建环境（`phprt buildimages`）

**本文是独立文档，不参与 `docs/` 设计文档体系，也不修改已有文档。**
已有文档里基于 podman / docker 的容器方案与本文无关；本文只描述当前实现。

---

## 1. 它解决什么

宿主上编译依赖 gcc / make / perl / cmake 等工具。换机器、清环境就要重装。
OCI 构建环境把工具装进一个由 phprt 自己管理的 rootless 容器里，并在后续
`install` / `add` / `update` 中复用。

- 镜像由 phprt 从 Registry 拉取，不依赖 podman / docker。
- 容器由 Rust 的 `libcontainer`（youki 的库）管理。
- 镜像缓存在用户级目录，同一台机器的多个项目共享。
- 每个项目、每个产品系列有自己的可写实例，工具装一次后一直留在实例里。
- 容器内执行的仍然是同一个 `phprt` 二进制，构建逻辑、recipe、catalog、下载、
  state 全部复用。

---

## 2. 前置条件

- Linux + systemd（libcontainer 依赖 systemd 管理 cgroup）。
- cgroup v2。
- 普通用户可以创建 user namespace：`/etc/subuid`、`/etc/subgid` 有当前用户的
  映射，并且 `newuidmap` / `newgidmap` 可用。
- 镜像必须是 Linux、与宿主同架构，并且带 `dnf` 或 `apt-get`；当前只实测了
  Fedora 43（`quay.io/fedora/fedora:43`，dnf5）。
- 宿主的 `phprt` 二进制要能在镜像里运行：宿主 glibc 不能比镜像新。宿主和镜像
  同为 Fedora 43 时可以直接跑；宿主比镜像新会报 `GLIBC_2.xx not found`。
- 构建 `phprt` 本身不再依赖 libseccomp（已关闭该 feature），运行时也不要求镜像里
  有 `libseccomp.so.2`。

不需要：podman、docker、kiri、root 权限。

---

## 3. 快速开始

```bash
# 1. 拉取镜像（一次即可，跨项目共享）
phprt buildimages add quay.io/fedora/fedora:43 --name fedora43

# 2. 给产品系列选择构建环境
phprt buildimages use php@8.5 --image fedora43

# 3. 正常构建；第一次会在容器里自动装 gcc / make / cmake 等
phprt install php@8.5
./myphp-8.5 -v

# 4. 之后的 add / update 也自动沿用
phprt add php@8.5 swoole
phprt update php@8.5
```

第一次也可以不单独 `use`，直接：

```bash
phprt install php@8.5 --image fedora43
```

`--image` 写在本次命令上，选择会写进该系列的 `state.json`，之后不需要再带。

---

## 4. 命令

### 4.1 `phprt buildimages add`

```text
phprt buildimages add <镜像引用> [--name <别名>] [--offline]
```

- 拉 manifest、config 和层，校验层 digest，解包到用户级缓存。
- 记录 manifest digest；`--name` 起一个本地别名，之后可以代替 digest。
- 同一个 tag 重新 `add` 时，如果 digest 变了，会缓存成新条目，旧内容保留；
  tag 指向最新 digest。
- `--offline` 只在本地已有缓存时成功，否则报错。
- 只支持 gzip / 未压缩 tar 层；zstd 层会报错。
- 支持多层镜像：whiteout 先作用于下层，再解包本层条目；目标名为空、`.`、`..`
  的非法 whiteout 直接报错，不会删除父目录。
- 文件与目录互换的条目会先删旧路径再重建，符合 OCI 的 changeset 语义。
- 解包时的 chmod 和 whiteout 删除会检查路径仍在 rootfs 内；通过符号链接逃出
  rootfs 的层会报错。
- 会检查镜像里有 `/bin/sh` 和 `dnf` / `apt-get`。
- 不创建容器实例；实例在第一次 `install` / `plan` 时按项目创建。

### 4.2 `phprt buildimages list`

列出本地缓存的镜像：digest 前 12 位、别名、平台、创建时间、大小、原始引用。
不联网。

### 4.3 `phprt buildimages use`

```text
phprt buildimages use <产品>@<系列> --image <digest|别名|引用>
phprt buildimages use <产品>@<系列> --host
```

- 把选择写进 `.phprt/<产品>-<系列>/state.json` 的 `build` 段。
- 该系列还没有方案时，会按 bundle 的默认组件初始化一份方案；效果和第一次
  `install` 的默认方案一致。
- `--host` 删除 `build` 段，回到宿主构建。
- 已经安装过的系列切换环境时，必须加 `--force`；否则报错，避免悄悄重编。
- 只写选择，不创建容器；下一次 `install` / `plan` 才建实例。
- 只支持本地模式，不能和 `--global --prefix` 一起用。

### 4.4 `phprt install` / `add` / `update` / `plan --image`

```text
phprt install php@8.5 --image fedora43
phprt add php@8.5 swoole --image fedora43
phprt update php@8.5 --image fedora43
phprt plan php@8.5 --image fedora43
```

- `install` / `add` / `update` 会把选择写进 state，然后进入容器执行。
- `plan` 只预览：使用镜像里的工具，不写 state、不装构建工具。
- state 里已经有镜像时，后续命令不需要再带 `--image`。
- 已安装的系列换镜像时，`install` 需要 `--force`；`add` / `update` 没有这个语义，
  会提示先用 `buildimages use --force`。
- `--downloads` 仍然只下载源码，不触发工具安装。
- `--offline` 下不拉镜像、不装包；镜像和工具必须已经准备好。

### 4.5 `phprt buildimages rm` / `gc`

```text
phprt buildimages rm <digest|别名|引用> [--force]
phprt buildimages gc [--dry-run]
```

- `rm` 删除镜像目录（rootfs + 层）和索引条目。
- 当前项目有 state 引用该 digest 时拒绝，`--force` 才删。
- 其他项目是否引用无法全盘判断；`--force` 只提示，不跨项目扫描。
- 已有容器实例不会被自动删除：实例有自己的 rootfs，镜像删了仍能继续用，
  但不能再从镜像重建。
- `gc` 清理两类东西：索引里没有的镜像目录；项目根已经不存在的实例。
  `--dry-run` 只列出。

---

## 5. 存储布局

镜像和实例放在用户级目录，默认：

```text
~/.local/share/phprt/buildimages/          # 遵循 XDG_DATA_HOME
├── index.toml                             # digest / 引用 / 别名 / 平台 / 大小
├── images/<digest-hex>/
│   ├── rootfs/                            # 解包后的文件系统
│   ├── layers/                            # 原始压缩层
│   ├── config.json                        # 镜像 config blob
│   ├── manifest.json
│   ├── image.json                         # ref / digest / 层清单 / 平台
│   └── modes.json                         # 解包时为复制临时放宽的 mode（files / dirs）
└── instances/<实例 id>/
    ├── instance.json                      # 镜像、项目根、创建时间、挂载/二进制指纹
    └── bundle/
        ├── config.json                    # OCI runtime spec
        └── rootfs/                        # 实例自己的可写 rootfs（含已装工具）
```

运行时状态（libcontainer 的容器状态、pid、notify socket）放在：

```text
$XDG_RUNTIME_DIR/phprt/buildimages/state/<实例 id>/
```

重启后这部分会消失，但 `instances/<id>/bundle/rootfs` 还在；下一次命令会从同一个
rootfs 重新创建容器，工具不用重装。

环境变量 `PHPRT_BUILDIMAGES_DIR` 可以覆盖用户级根目录，主要用于测试。

项目里只多一个 state 字段，没有新的项目级状态文件：

```json
"build": {
  "mode": "oci",
  "image": "sha256:c9fbab…",
  "image_ref": "quay.io/fedora/fedora:43",
  "runtime": "youki",
  "instance": "phprt-9d412bf8-php-8.5-c9fbabbeee0e",
  "instance_created": ""
}
```

- `image` 是 manifest digest，不是 tag。
- `instance` 只是缓存；实例目录或容器对不上就重建。
- 没有 `build` 字段表示宿主构建。

---

## 6. 一次命令怎么执行

以 `phprt install php@8.5`（state 已选择镜像）为例：

```text
宿主 phprt
  1. 读 state，确认使用哪个 digest
  2. 镜像不在本地缓存：交互式询问是否拉取；非交互直接报错
  3. 计算实例 id：项目 hash + 产品 + 系列 + digest 短串
  4. 实例建过且 rootfs 在：复用；否则从镜像复制 rootfs
  5. 生成 OCI spec：项目根、download_dir、build_dir、内部目录、recipes（只读）、
     /phprt、/etc/resolv.conf；共享宿主网络
  6. 创建 / 复用 libcontainer 实例（rootless + systemd cgroup）
  7. 探测 /phprt --version，确认二进制能在镜像里跑
  8. 把整条命令转发进容器
        /phprt <原命令>，环境变量 PHPRT_IN_CONTAINER=1
容器里的 phprt
  9. 正常执行 install / add / update / plan；看到 PHPRT_IN_CONTAINER 就不再转发
 10. 发现缺构建命令：读 plan 的 host_commands，把安装命令去掉 sudo、补 -y
     （apt 先 apt-get update），在容器里执行；复检一次后继续
 11. 编译、写 state、写入口
```

`PHPRT_IN_CONTAINER` 是内部标记，不出现在帮助里，用户不需要手动设置。

### 6.1 自动装包

宿主模式下缺工具只提示安装命令；容器模式下会直接执行。命令来源是现有的
`plan` 输出：

```json
"host_commands": {
  "pm": "dnf",
  "missing": ["gcc", "g++", "make", "perl"],
  "install": "sudo dnf install gcc gcc-c++ make perl"
}
```

实际执行时变成 `dnf install -y gcc gcc-c++ make perl`（去掉 `sudo`，补 `-y`）；
apt 镜像会先执行 `apt-get update`。装完仍然缺就报错，不循环安装。

`--offline` 时不执行这一步，缺什么直接报错。

### 6.2 为什么整条命令转发

- 宿主和容器里是同一个二进制，容器里的 `ctx["host"]`、路径、属主天然一致。
- 只有容器里的 `plan` 知道容器实际有哪些工具；宿主不用猜。
- catalog、下载、解压、编译仍然用原来的代码，容器只是执行环境。
- 入口不需要第二套协议或服务。

---

## 7. 签名与 `scheme --lock`

镜像 digest 会进构建签名：切换镜像后，已有节点会按签名重编，不会复用另一套
libc 编出来的产物。容器 id 不进签名；容器重建不触发重编。

`phprt scheme --lock` 会在导出的命令后面追加 `--image <digest>`，并打印一行准备
镜像的注释：

```text
# 目标机器先准备镜像：phprt buildimages add quay.io/fedora/fedora:43
phprt install php@8.5.13 '...' --targets 'cli' --image sha256:c9fbab…
```

在另一台机器上：

```bash
phprt buildimages add quay.io/fedora/fedora:43   # 会校验 digest
phprt install php@8.5.13 '...' --targets 'cli' --image sha256:c9fbab…
```

实例和已装工具不导出；目标机器会新建实例并重新装工具。

---

## 8. 各情况的处理

| 情况 | 行为 |
| --- | --- |
| state 有 digest，镜像缓存缺失 | 交互式问是否拉取，默认否；非交互报错并提示 `buildimages add` |
| 镜像缓存存在，实例 rootfs 缺失 | 自动从镜像复制 rootfs，重建容器 |
| 宿主重启，运行时状态丢失 | 自动用同一个实例 rootfs 重建容器；工具保留 |
| 实例存在，工具被删 | 下一次 `install` / `add` 重新发现缺包并安装 |
| tag 指向了新 digest | state 里的 digest 是权威；`add` 会把新内容缓存成新条目，旧内容保留 |
| state digest 与 tag 当前 digest 不一致 | 报错，要求显式 `buildimages use --image <新 digest> --force` |
| 已安装的系列切换镜像 | 需要 `--force`；否则拒绝 |
| `phprt` 在镜像里跑不起来 | 报错并提示换 glibc 不低于宿主的镜像 |
| 镜像没有 dnf / apt-get | `buildimages add` 时报错 |
| `--offline` | 不拉镜像、不装包；已有镜像和工具可以离线构建 |
| `--global --prefix` | 构建镜像只支持本地模式；`install` 会报错 |

---

## 9. 限制

- 不是安全沙箱：容器共享宿主网络（否则 dnf 无法联网），rootless 用户命名空间
  只做 UID/GID 映射。不要用它运行不可信镜像。
- 没有 overlayfs：每个实例复制一份 rootfs（Fedora 43 约 180 MB）；镜像本身
  压缩层 + rootfs 约 250 MB。
- 只支持 Linux、同架构、gzip / 未压缩 tar 层。zstd 层、跨架构镜像报错。
- Registry 只做了匿名拉取；私有仓库、需要登录的镜像暂不支持。
- 硬链接、文件与符号链接的 mtime 会复制到实例；目录 mode 在实例复制完成后恢复
  （如 `/usr/bin` 会回到 `0555`），宿主删除实例前会临时放宽目录权限。
- whiteout 按 OCI 规范先作用于下层，再解包本层条目；非法 whiteout 会报错。
- 解包时的 chmod 和 whiteout 删除会做 rootfs 边界检查；通过符号链接逃出
  rootfs 的层会被拒绝。
- 解包跳过设备节点，不应用 xattr / file capability；Fedora 43 基础镜像实测可用。
- 镜像里的 `/etc/resolv.conf` 由宿主只读挂入；镜像自带的会被覆盖。
- dnf 已实测；apt 路径已实现（先 update、补 `-y`）但没有在 Debian / Ubuntu 基础
  镜像上验收。
- 删除镜像不会删除已有实例；也不做跨项目引用统计。`rm --force` 前需自行确认。
- `gc` 用“项目根路径是否还存在”判断实例是否孤儿；项目放在临时挂载点上时，
  挂载没挂上会被误判，先 `gc --dry-run` 确认。
- 没有镜像/实例的并发锁；同时启动两个 phprt 去拉同一个镜像或建同一个实例，
  行为未定义。
- 只有 `install` / `add` / `update` / `plan` 会按 state 进入容器；
  `list` / `scheme` / `graph` / `audit` / `catalog` 仍在宿主上执行。
- `buildimages` 子命令不能和 `--global --prefix` 的 `use` / `rm` 一起用。

---

## 10. 故障排查

**`error: phprt 在构建镜像里跑不起来`**

宿主二进制要求比镜像新的 glibc。换同代镜像（如宿主 Fedora 43 就用
`quay.io/fedora/fedora:43`），或换 glibc 不低于宿主的镜像。

**容器创建失败 / systemd scope 报错**

确认宿主是 systemd、cgroup v2，并且 `/etc/subuid`、`/etc/subgid` 有当前用户；
`newuidmap`、`newgidmap` 在 PATH 上。残留的 failed scope 不影响新建，可以用
`systemctl --user reset-failed 'youki-*'` 清理。

**镜像层下载失败**

先重试 `phprt buildimages add <引用>`。需要代理时，在宿主环境里设置
`HTTPS_PROXY` / `HTTP_PROXY`；phprt 的 registry 客户端会使用它们。

**`error: 层里有非法 whiteout ...`**

镜像层里的 whiteout 目标名为空、`.` 或 `..`，不符合 OCI 规范。换一个镜像，
或修正该层后重新 `add`。

**`error: 层里的路径通过符号链接逃出了 rootfs`**

层试图借符号链接修改或删除 rootfs 外的文件。phprt 会拒绝整个镜像，不会继续
解包。确认镜像来源可信后再重试。

**镜像在本地，但 install 报“不在本地”**

检查 `phprt buildimages list`。state 里的 digest 必须和缓存条目一致；tag 前进
后旧的 digest 仍可单独使用，不会被覆盖。

**想回到宿主构建**

```bash
phprt buildimages use php@8.5 --host --force   # 已安装时加 --force
phprt install php@8.5                          # 按宿主签名重编
```

**清理空间**

```bash
phprt buildimages list
phprt buildimages gc --dry-run
phprt buildimages gc
phprt buildimages rm <digest> --force
```

实例目录 `~/.local/share/phprt/buildimages/instances/<id>` 可以手工删除；
下一次命令会按需要重建。

---

## 11. 已验证的流程

2026-11-23 在 Fedora 43 宿主上实际跑通：

```text
phprt buildimages add quay.io/fedora/fedora:43 --name fedora43
phprt buildimages use php@8.5 --image fedora43
phprt install php@8.5
./myphp-8.5 -v                 # PHP 8.5.13 (NTS)
phprt scheme php@8.5 --lock    # 输出带 --image 和准备镜像的注释
```

- 首次在容器里自动执行 `dnf install -y gcc gcc-c++ binutils cmake m4 make perl pkgconf`。
- 10 个节点（含从源码构建的 patchelf）约 5 分钟完成。
- 产物在宿主上可直接运行，`php -m` 含 `openssl` / `curl` / `pdo_sqlite`。
- 文件属主是宿主用户；`sudo` 等 setuid 位保留。
- 实例 rootfs 的 `/usr/bin` 回到 `0555`，`/etc/shadow` 回到 `0000`；
  硬链接（如 getconf、zoneinfo）和 mtime 与镜像缓存一致。
- 删除运行时状态后从实例 rootfs 重建，工具仍在。

另外用合成层验证了解包器：opaque / 普通 whiteout 先于本层条目应用、非法
whiteout 直接报错且不删父目录、符号链接越界被拒绝、文件与目录互换能重建、
硬链接和 mtime 能复制到实例、目录 mode 能恢复。
