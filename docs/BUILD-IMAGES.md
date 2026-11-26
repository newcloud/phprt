# OCI 构建环境（`phprt buildimages`）

状态：已实现。本文描述构建环境（用 OCI 镜像里的工具编译）；把安装树产出为运行镜像
仍是方案，见 [plan/RUNTIME-IMAGE.md](plan/RUNTIME-IMAGE.md)。与代码冲突时以代码
实际行为为准。

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
- cgroup v2（已验证；libcontainer 也编进了 v1 支持，但未实测）。
- 普通用户可以创建 user namespace；实例只映射当前 uid/gid（容器内是 0）。
  多数发行版无需额外配置；如果报 user namespace / id mapping 错误，确认
  `/etc/subuid`、`/etc/subgid` 有当前用户的映射，并且 `newuidmap` /
  `newgidmap` 在 PATH 上。
- 镜像必须是 Linux、与宿主同架构，并且带 `dnf` 或 `apt-get`；已实测 Fedora 43
  （`quay.io/fedora/fedora:43`，dnf5）和 Debian 13（`docker.io/library/debian:13`，apt）。
- Debian / Ubuntu 需要 apt 沙箱设置；phprt 按镜像家族自动写进实例，见 §9。
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

# 2. 用 fedora43 建一个全局实例，并起名
phprt install php@8.5 --image fedora43 --instance-name fedora43mybuild_os

# 3. 之后的 add / update / install 沿用 state 里的实例
phprt add php@8.5 swoole
phprt update php@8.5

# 4. 另一个产品复用同一个实例
phprt install nginx@1.29 --instance fedora43mybuild_os
```

`--image` + `--instance-name` 写在本次命令上，选择会写进该系列的 `state.json`，
之后不需要再带。实例是用户级全局对象，名字全局唯一；不给 `--instance-name`
时默认名是 `<产品>-<系列>-<项目hash8>`。换基础镜像必须新建实例。
`phprt buildimages list` 同时列出镜像和实例。

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
- 命中镜像家族时会显示家族、设置和原因；家族按镜像内容匹配，见 §9。
- 不创建容器实例；实例在第一次 `install` / `plan` 时创建。实例是全局对象，
  名字由 `--instance-name` 或默认规则决定；`--instance` 只选择已有实例。

### 4.2 `phprt buildimages list`

列出本地缓存的镜像和全局实例，不联网。

- 镜像表：digest 前 12 位、别名、家族、平台、创建时间、大小、使用该 digest 的
  实例、原始引用；digest 前缀可以直接用在 `--image` / `rm` 上。家族读不出时显示 `-`。
- 实例表：name、image、owner、used_by、size、running/stopped。`used_by` 通过
  各项目 `.phprt/*/state.json` 汇总；owner 项目根不存在时标 `missing`。

### 4.3 `phprt buildimages use`

```text
phprt buildimages use <产品>@<系列> --instance <名字>
phprt buildimages use <产品>@<系列> --image <digest|digest前缀|别名|引用> [--instance-name <名字>]
phprt buildimages use <产品>@<系列> --host
```

- `--instance` 选择已有的全局实例；`--image` 选择镜像，`--instance-name` 给
  新实例起名。`--image` / `--instance` / `--host` 三选一，`--instance-name` 只能
  和 `--image` 一起用。`--image` 显式给出且本地没有时自动拉取。
- 把选择写进 `.phprt/<产品>-<系列>/state.json` 的 `build` 段。
- 只写选择，不创建容器；下一次 `install` / `plan` 才建实例。
- 该系列还没有方案时，会按 bundle 的默认组件初始化一份方案；效果和第一次
  `install` 的默认方案一致。
- `--host` 删除 `build` 段，回到宿主构建。
- 已经安装过的系列切换环境时，必须加 `--force`；否则报错，避免悄悄重编。
- 只支持本地模式，不能和 `--global --prefix` 一起用。

### 4.4 `phprt install` / `add` / `update` / `plan`

```text
phprt install php@8.5 --image fedora43 --instance-name fedora43mybuild_os
phprt install nginx@1.29 --instance fedora43mybuild_os
phprt add php@8.5 swoole --instance fedora43mybuild_os
phprt update php@8.5 --instance fedora43mybuild_os
phprt plan php@8.5 --instance fedora43mybuild_os
```

- `--image <digest|digest前缀|别名|引用>`：新建实例的基础镜像；显式给出且本地没有时自动
  拉取；`install` / `update` / `add` 加 `--offline` 时报错（`plan` 没有 `--offline`）。
- `--instance <名字>`：使用已有全局实例，必须存在。可以同时给 `--image`，但
  digest 必须一致。
- `--instance-name <名字>`：新建实例时使用的名字；同名但 digest 不同直接报错。
- state 里已经有实例时，后续命令不需要再带参数；实例 rootfs 在时不要求镜像
  缓存，只有实例 rootfs 丢失时才解析/拉取镜像重建。
- `install` / `add` / `update` 会把选择写进 state，然后进入容器执行。
- `plan` 只预览：不写项目的 `state.json`、不装构建工具；但会像 `install` 一样创建或
  复用实例，再把命令转发进去取结果。文本输出会先显示命中的镜像家族和设置，
  `--json` 不输出这一行。
- 已安装的系列换镜像时，`install` 需要 `--force`；`add` / `update` 没有这个语义，
  会报错，需先用 `buildimages use --force` 或 `install --force`。
- `install` / `update` / `add` 的 `--downloads` 仍然只下载源码，不触发工具安装；
  `plan` 不接受 `--downloads`。
- `install` / `update` / `add` 的 `--offline` 下不拉镜像、不装包；镜像和工具必须已经
  准备好。

### 4.5 `phprt buildimages rm` / `rm-instance`

```text
phprt buildimages rm <digest|digest前缀|别名|引用> [--force]
phprt buildimages rm-instance <名字> [--force]
```

- `rm-instance`：先拿实例锁，停掉/删除 youki 容器，再删除实例目录。实例正被
  另一个 phprt 使用时拒绝；项目 state 引用该实例时默认拒绝，`--force` 才删。
- `rm`：只要还有实例使用该 digest，就拒绝，且不能用 `--force` 绕过；必须先
  `rm-instance`。只有项目 state 引用、没有实例时，默认拒绝，`--force` 可删；
  下次 install 会自动拉取。
- 删除镜像只删镜像目录和索引条目；不会再出现“实例还引用已删镜像”的状态。
- 没有 `gc`；孤儿实例和残留目录都通过显式 `rm-instance` / `rm` 处理。

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
    ├── instance.json                      # name / image / owner / projects / 挂载 / phprt 二进制标识
    ├── .lock                              # 实例锁；同一实例串行使用
    └── bundle/
        ├── config.json                    # OCI runtime spec
        └── rootfs/                        # 实例自己的可写 rootfs（含已装工具）
```

新实例 id 是 `phprt-<名字>`；旧实例仍保留旧 id
`phprt-<项目hash8>-<产品>-<系列>-<digest12>`，第一次使用时收编成用户可见名字。
`instance.json` 里的二进制标识由 phprt 可执行文件的路径、大小和 mtime 组成；
它或挂载列表变了，只重建运行时容器，rootfs 和已装工具保留。

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
  "instance": "fedora43mybuild_os"
}
```

- `image` 是 manifest digest，不是 tag。
- `instance` 是全局实例名，不是内部 id。
- 没有 `build` 字段表示宿主构建。
- 实例绑定一个基础镜像 digest；同名实例不能换镜像，换镜像要新建名字。

---

## 6. 一次命令怎么执行

以 `phprt install php@8.5`（state 已选择镜像）为例：

```text
宿主 phprt
  1. 读 state，拿到实例名和 image digest
  2. 实例 rootfs 在：直接复用，不要求镜像缓存；rootfs 缺失才解析/拉取镜像
  3. 实例 id 是 phprt-<名字>；没有名字时用 <产品>-<系列>-<项目hash8>
  4. 实例没建过：从镜像复制 rootfs；建过：复用
  5. 项目切了：保留 rootfs，只重启运行时容器并换挂载
  6. 拿实例锁；同一实例同时只允许一个 phprt，第二个等待
  7. 生成 OCI spec：项目根、download_dir、build_dir、内部目录、备份目录、
     配置文件（只读）、recipes（只读）、/phprt、/etc/resolv.conf；共享宿主网络
  8. 创建 / 复用 libcontainer 实例（rootless + systemd cgroup）
  9. 探测 /phprt --version，确认二进制能在镜像里跑
 10. 把整条命令转发进容器
        /phprt <原命令>，环境变量 PHPRT_IN_CONTAINER=1
容器里的 phprt
 11. 正常执行 install / add / update / plan；看到 PHPRT_IN_CONTAINER 就不再转发
 12. 发现缺构建命令：读 plan 的 host_commands，把安装命令去掉 sudo、补 -y
     （apt 先 apt-get update），在容器里执行；复检一次后继续
 13. 编译、写 state、写入口
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
apt 镜像会先执行 `apt-get update`，并按 §9 的家族设置在实例里关掉 apt 的降权沙箱。
装完仍然缺就报错，不循环安装。

`--offline` 时不执行这一步，缺什么直接报错。

### 6.2 为什么整条命令转发

- 宿主和容器里是同一个二进制，容器里的 `ctx["host"]`、路径、属主天然一致。
- 只有容器里的 `plan` 知道容器实际有哪些工具；宿主不用猜。
- catalog、下载、解压、编译仍然用原来的代码，容器只是执行环境。
- 入口不需要第二套协议或服务。

---

## 7. 签名与 `scheme` 导出

镜像 digest 会进方案指纹和节点签名：切换镜像后，已有节点会按签名重编，不会复用
另一套 libc 编出来的产物。实例名和容器 id 都不进指纹；同名实例换项目、容器重建都不
触发重编。两个实例 digest 相同时互相切换也不会重编。

`phprt scheme`（带不带 `--lock` 都一样）会在导出的命令末尾追加 `--image <digest>`，
并打印一行准备镜像的注释：

```text
# 目标机器先准备镜像：phprt buildimages add quay.io/fedora/fedora:43
phprt install php@8.5.13 '...' --targets 'cli' --image sha256:c9fbab…
```

在另一台机器上：

```bash
phprt buildimages add quay.io/fedora/fedora:43   # 会校验 digest
phprt install php@8.5.13 '...' --targets 'cli' --image sha256:c9fbab…
```

实例和已装工具不导出；导出的命令只带 `--image`，不带实例名。目标机器会按默认名
新建实例并重新装工具。

---

## 8. 各情况的处理

| 情况 | 行为 |
| --- | --- |
| state 有实例，rootfs 在 | 直接复用，不要求镜像缓存 |
| state 有实例，rootfs 缺失 | 按实例 meta 的 image_ref 自动拉取镜像并复制 rootfs；`--offline` 或镜像缺失时报错 |
| 实例不存在，显式 `--image` | 自动拉取（`--offline` 除外），按 `--instance-name` 或默认名新建 |
| 项目切换 | 保留 rootfs 和工具，只重启运行时容器并换挂载 |
| 同名实例遇到不同 digest | 报错，要求用 `--instance-name` 新建；不删 rootfs |
| 实例被另一个 phprt 使用 | 等待并打印提示；`rm-instance` 直接拒绝 |
| state 有 digest，镜像缓存缺失 | 实例 rootfs 在就不管；需要重建时自动拉取 |
| tag 指向了新 digest | state 里的 digest 是权威；显式换镜像要新实例名 |
| 已安装的系列切换镜像 | 需要 `--force`；否则拒绝 |
| `rm` 镜像时仍有实例使用 | 拒绝，不能用 `--force` 绕过；先 `rm-instance` |
| `rm-instance` 时 state 仍引用 | 默认拒绝，`--force` 才删；下次命令按镜像重建 |
| `phprt` 在镜像里跑不起来 | 报错并提示换 glibc 不低于宿主的镜像 |
| 镜像没有 dnf / apt-get | `buildimages add` 时报错 |
| apt 镜像（Debian / Ubuntu） | 按镜像家族写入 apt 沙箱设置，见 §9 |
| 镜像同时命中多个家族 | 报错，要求收紧 `id_like` |
| `--offline` | 不拉镜像、不装包；已有镜像和工具可以离线构建 |
| `--global --prefix` | 构建镜像只支持本地模式；`install` 会报错 |

---

## 9. 镜像家族与 apt 沙箱

镜像家族按**镜像内容**识别，不按引用或 digest。`recipes/images.toml` 的
`[families.<名字>]` 只写事实和必要设置：

```toml
[families.debian]
pm = "apt"
id_like = ["debian", "ubuntu"]
reason = """
rootless 用户命名空间只映射了 uid/gid 0，libcontainer 还会写 setgroups=deny。
apt 默认降权到 _apt(42) / nogroup(65534) 做下载沙箱，这两个 id 在命名空间里
没有映射，setgroups / setegid / seteuid 全部失败，apt-get update 直接报错退出。
把 APT::Sandbox::User 设成 root 后 apt 不再降权，下载和装包才能进行；
容器本身仍是 rootless，命令只在用户命名空间内以 uid 0 运行。
"""
apt_sandbox = "root"
```

- `pm`：`apt` 或 `dnf`，必须和镜像里的包管理器一致。
- `id_like`：和镜像 `/etc/os-release` 的 `ID` / `ID_LIKE` 取交集；省略或为空
  表示只按 `pm` 匹配。大小写不敏感。
- `reason`：有设置时必填，写在配置里；`buildimages add` / `use` 会原样显示。
- `apt_sandbox`：目前只接受 `"root"`。

同时命中多个家族是配置错误，phprt 直接报错，不会按名字挑一个。未知字段、
未知 `pm`、空 `id_like` 项、缺 `reason`、未知设置值都会让 recipe 加载失败。

### 9.1 为什么 Debian / Ubuntu 需要 `apt_sandbox = "root"`

rootless 容器只有一个用户命名空间，libcontainer 只映射 uid/gid 0，并写入
`setgroups=deny`。apt 默认把下载和校验降权到 `_apt`（uid 42）和 `nogroup`
（gid 65534）执行沙箱；这两个 id 在命名空间里没有映射，于是：

```text
E: setgroups 65534 failed - setgroups (1: Operation not permitted)
E: setegid 65534 failed - setegid (22: Invalid argument)
E: seteuid 42 failed - seteuid (22: Invalid argument)
E: Method http has died unexpectedly!
```

`apt-get update` 直接失败，后续自动装 gcc / make 等也无法进行。

**没有采用** subuid/subgid 映射 `_apt` / `nogroup`：libcontainer 对 rootless
容器固定写 `setgroups=deny`，即使有 `/etc/subuid` / `/etc/subgid`，apt 仍然
会 `setgroups` 失败；而且 `_apt` 写出的文件会落在 subuid 范围，宿主侧清理
实例时需要额外的权限处理。把 `APT::Sandbox::User` 设成 `root` 只取消 apt 的
降权，容器本身仍是 rootless，命令仍只在用户命名空间内以 uid 0 运行。

### 9.2 设置写在哪里

创建实例后，如果镜像命中带 `apt_sandbox = "root"` 的家族，phprt 会写入：

```text
<实例 rootfs>/etc/apt/apt.conf.d/99phprt-rootless-sandbox
APT::Sandbox::User "root";
```

- 只写实例 rootfs，不改镜像缓存，不影响宿主。
- 文件已存在且内容相同就不重复写；内容不对会覆盖。
- 每次 `install` / `add` / `update` / `plan` 建实例后都会检查，实例重建后会重新写。
- `buildimages add` 只识别家族并显示，不创建实例，也不写这个文件。

可以在容器里确认：

```bash
phprt __container-exec --instance <实例 id> --workdir <项目根> \
  --env PHPRT_IN_CONTAINER=1 -- sh -c 'apt-config dump | grep -i sandbox'
# APT::Sandbox "";
# APT::Sandbox::User "root";
```

### 9.3 输出

`phprt buildimages add` / `use` 会显示命中的家族、设置和 `reason`：

```text
家族      debian
设置      apt_sandbox=root
原因      rootless 用户命名空间只映射了 uid/gid 0，libcontainer 还会写 setgroups=deny。
          ...
```

`phprt buildimages list` 的镜像表带 family，并且多一列使用该 digest 的实例：

```text
digest         alias  family  platform     created          size  instances  refs
bfc36aecdece          debian  linux/amd64  2026-11-04...  165.5M  php-8.5    docker.m.daocloud.io/library/debian:13
```

实例表单独列出 name、image、owner、used_by、size、running/stopped。读不出镜像内容时
family 显示 `-`。

`phprt plan` 在文本输出第一行显示命中的家族和设置；`--json` 不输出这一行，
保持 JSON 可解析。

---

## 10. 限制

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
- dnf 与 apt 都已实测；Debian / Ubuntu 依赖 §9 的 apt 沙箱设置，没有该设置时
  `apt-get update` 会在 `setgroups` / `setegid` 处失败。
- 构建镜像决定产物的运行库；关掉 nginx 的 `xcrypt` 时，Debian 构建的 nginx 在
  Fedora 宿主可能缺 `libcrypt.so.1`，见 §10.1。
- rootless 用户命名空间只映射 uid/gid 0；容器内改不了 `_apt` / `nogroup` 的
  uid/gid，也不支持按 subuid 范围给单个用户做映射。
- 删除镜像不会自动删除实例；只要还有实例使用该 digest，`rm` 就拒绝，必须先
  `buildimages rm-instance`。不做跨项目引用统计，state 引用用 `--force` 放行。
- 实例是全局共享的：rootfs 和已装工具共用；项目源码、`.phprt`（含 `.phprt/build`）仍挂
  当前项目。同一实例同时只允许一个 phprt 使用，第二个会等待实例锁释放；要并行
  编译请用不同实例。
- 镜像拉取没有跨进程锁；同一实例的创建、使用和删除有 flock。
- 项目目录搬家会改变默认实例名里的项目 hash，但 state 里已经写下的实例名仍按
  全局名字解析。
- 只有 `install` / `add` / `update` / `plan` 会按 state 进入容器；
  `list` / `scheme` / `graph` / `audit` / `catalog` 仍在宿主上执行。
- `buildimages use` / `rm` / `rm-instance` 不能和 `--global --prefix` 一起用。

### 10.1 构建镜像决定产物的运行库

phprt 只把构建时找到的**非系统库**收进 `<tree>/lib/phprt`，例如 pcre2、openssl、
zlib；glibc 这类基础系统库仍然由运行环境提供。libxcrypt 默认由 nginx 静态链入，只有
关掉 `xcrypt` feature 时才改为链接系统 libcrypt。所以**产物能直接在哪个发行版上跑，
取决于构建镜像和目标宿主是不是同一套运行库**。

实测（2026-11-24，Fedora 43 宿主，Debian 13 实例）：

- 关掉 nginx 的 `xcrypt` feature 时，Debian 13 实例构建的 nginx 链接
  `libcrypt.so.1`（Debian 的 libxcrypt）；
- Fedora 43 只有 `libcrypt.so.2`，在宿主直接运行报：

  ```text
  error while loading shared libraries: libcrypt.so.1: cannot open shared object file
  ```

- 同一个二进制在 Debian 13 实例里运行正常：

  ```text
  nginx version: nginx/1.29.8
  built by gcc 14.2.0 (Debian 14.2.0-19)
  built with OpenSSL 3.5.9 29 Sep 2026
  ```

nginx 1.29 的默认做法是开 `xcrypt` feature：phprt 自己编译 libxcrypt 4.5.2
（`recipes/dep/libxcrypt.star`，只装 `libcrypt.a`），nginx 的 `-lcrypt` 命中静态库，
产物里没有 `DT_NEEDED libcrypt.so.1`。默认值写在
`recipes/bundles/nginx/1.29.toml` 的 `features = ["xcrypt"]`；删掉这一行就回到上面的
系统 libcrypt 行为。开关只在 recipe / bundle 层，没有 CLI 参数。

为什么会缺：

- `libcrypt.so.1` **不是** OpenSSL 的 `libcrypto.so.3`。前者是 `crypt(3)` 密码哈希库
  （libxcrypt），后者是 OpenSSL 的加密库；名字像，但是两个东西。
- 关掉 `xcrypt` 时，nginx 的 HTTP Basic auth 会调用 `crypt()`，Debian 镜像的
  configure 找到它并链接 `-lcrypt`，ELF 里记录的是 `DT_NEEDED libcrypt.so.1`。
- 关掉 `xcrypt` 时，libcrypt 就是系统库：phprt 只把构建时解析到的**非系统库**收进
  `<tree>/lib/phprt`，例如 openssl、pcre2、zlib；glibc、libcrypt 交给运行环境提供。
  默认开启 `xcrypt` 时，libxcrypt 是静态 dep，不会出现在产物的 `NEEDED` 里。
- 实例只是构建沙箱。实例里的 `/lib/x86_64-linux-gnu/libcrypt.so.1` 不会复制到产物树；
  产物在宿主运行时用的是宿主的 loader 和宿主的系统库。
- Debian 13 的 libxcrypt SONAME 是 `libcrypt.so.1`，Fedora 43 是 `libcrypt.so.2`。
  宿主 loader 只按 ELF 里记录的名字找，所以 Fedora 上缺 `libcrypt.so.1`；
  同一个二进制在实例里能找到实例自己的 `libcrypt.so.1`，所以 `nginx -V` 正常。

关掉 `xcrypt` 时，`ldd` 能直接看到缺失的 libcrypt，以及哪些库是随产物带的：

```text
libcrypt.so.1   => not found
libssl.so.3     => <tree>/lib/phprt/libssl.so.3
libcrypto.so.3  => <tree>/lib/phprt/libcrypto.so.3
libpcre2-8.so.0 => <tree>/lib/phprt/libpcre2-8.so.0
libz.so.1       => <tree>/lib/phprt/libz.so.1
libc.so.6       => /lib64/libc.so.6
```

处理方式：

1. **让产品自己带 libcrypt**。nginx 1.29 已经这么做：`xcrypt` 默认把 libxcrypt
   静态链进 `sbin/nginx`，产物不依赖宿主 `libcrypt.so.1`。其他用 `crypt()` 的产品
   可以照这个 recipe 模式加：dep 只装静态库，产品 feature 带 `needs`。
2. **优先让构建镜像和宿主同族**。Fedora 宿主用 `quay.io/fedora/fedora:43`，
   产物直接链接宿主已有的运行库。
3. 关掉 `xcrypt`、又必须用 Debian / Ubuntu 镜像构建并在 Fedora 上运行时，宿主安装
   兼容包：`sudo dnf install libxcrypt-compat`（提供 `libcrypt.so.1`）。
4. 先诊断：`ldd <产物>` 看哪个库 `not found`；`file` / `objdump -T` 看 ELF 和
   符号版本。
5. 区分两个问题：`phprt` 自身二进制能在镜像里跑，只说明它需要的 GLIBC 符号版本
   不高于镜像；**不代表构建出来的产品能在宿主上跑**。产品依赖的是构建镜像里的
   系统库，不是 phprt 二进制的依赖。

---

## 11. 故障排查

**`error: phprt 在构建镜像里跑不起来`**

宿主二进制要求比镜像新的 glibc。换同代镜像（如宿主 Fedora 43 就用
`quay.io/fedora/fedora:43`），或换 glibc 不低于宿主的镜像。

**宿主运行产物时报 `libcrypt.so.1` / `GLIBC_2.xx` not found**

这是构建镜像和宿主运行库不一致，不是实例本身坏了。用 `ldd <产物>` 确认缺哪个库；
优先改用和宿主同族的镜像重新构建。Debian 构建物在 Fedora 上运行时，通常需要
`sudo dnf install libxcrypt-compat`。这只影响关掉 nginx `xcrypt` feature 的产物；
默认 `xcrypt` 已把 libxcrypt 静态链入。详见 §10.1。

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

**apt 报 `setgroups ... failed` / `setegid` / `seteuid`**

镜像命中了 `debian` 家族但实例里缺少 apt 沙箱设置。检查
`<实例 rootfs>/etc/apt/apt.conf.d/99phprt-rootless-sandbox` 是否存在，内容应为
`APT::Sandbox::User "root";`。再跑一次 `plan` / `install` 会自动补写；也可以
删掉实例目录让它重建。不要去改镜像缓存里的文件。

**`error: 镜像同时匹配多个家族`**

`recipes/images.toml` 的两个 `[families.*]` 都命中了该镜像（例如都写了
`pm = "apt"`，且 `id_like` 与镜像的 ID / ID_LIKE 有交集）。收紧 `id_like`，
让同一镜像只命中一个家族。

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
phprt buildimages list                    # 先看哪个实例在用哪个镜像
phprt buildimages rm-instance <名字>      # 先删实例；被 state 引用时加 --force
phprt buildimages rm <digest|别名|引用>   # 没有实例使用后才能删镜像
```

删除镜像时如果还有实例使用，会直接拒绝，`--force` 也不能绕过；先删实例。
实例正在被另一个 phprt 使用时，`rm-instance` 也会拒绝。

---

## 12. 已验证的流程

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

2026-11-23 在 Debian 13 基础镜像上实际跑通（宿主 Fedora 43）：

```text
phprt buildimages add docker.m.daocloud.io/library/debian:13
phprt buildimages use php@8.5 --image docker.m.daocloud.io/library/debian:13
phprt install php@8.5
./myphp-8.5 -v                 # PHP 8.5.13 (NTS)
```

- `buildimages add` / `list` / `use` 显示家族 `debian`、设置 `apt_sandbox=root`
  和配置里的原因。
- `plan` 文本第一行显示 `镜像家族  debian（apt_sandbox=root）`。
- 实例 rootfs 写入 `etc/apt/apt.conf.d/99phprt-rootless-sandbox`；容器里
  `apt-config dump | grep -i sandbox` 输出 `APT::Sandbox::User "root";`。
- 首次自动执行
  `apt-get update && apt-get install -y gcc g++ binutils cmake m4 make perl pkg-config`，
  不再出现 `setgroups` / `setegid` / `seteuid` 错误。
- 10 个节点编译完成，`./myphp-8.5 -v` 输出 PHP 8.5.13 (NTS)。

2026-11-23 全局实例流程实测：

- 旧实例 `phprt-4add57aa-php-8.5-bfc36aecdece` 自动收编为名字 `php-8.5`，
  rootfs 和已装工具不复制、不重装。
- `phprt buildimages list` 同时显示镜像和实例；实例的 `used_by` 显示 `php@8.5`。
- `phprt buildimages rm <debian digest>` 被拒绝，提示先删实例。
- `phprt buildimages rm-instance php-8.5` 因 state 仍引用而拒绝，列出 `php@8.5`。

2026-11-24 跨产品复用同一实例实测（Debian 13 实例，Fedora 43 宿主）：

```text
phprt install nginx@1.29 --instance php-8.5
# exit 0；zlib 1.3.2 和 nginx 1.29.8 编译完成
```

- nginx recipe 之前漏了 zlib，`./configure` 会报
  `the HTTP gzip module requires the zlib library`；已把 zlib 加进 `needs`、
  `LINES`、bundle `[deps]` 和 `--with-cc-opt` / `--with-ld-opt`。
- 构建完成后 `buildimages list` 的 `used_by` 显示 `nginx@1.29,php@8.5`。
- 在实例里执行 `nginx -V` 正常；关掉 `xcrypt` 时在 Fedora 宿主直接执行会缺
  `libcrypt.so.1`，原因和处理见 §10.1。
- 用 `plan --instance-name test-instance` 新建实例、再 `rm-instance test-instance`
  删除成功；`rm-instance` 在实例锁被占用时拒绝。

2026-11-24 nginx `xcrypt` feature 实测（Debian 13 实例，Fedora 43 宿主）：

```text
# bundle: features = ["xcrypt"]
phprt install nginx@1.29 --instance php-8.5 --force
readelf -d .phprt/nginx-1.29.8/sbin/nginx | grep NEEDED
# 只有 libpcre2-8 / libssl / libcrypto / libz / libc；没有 libcrypt
./.phprt/nginx-1.29.8/sbin/nginx -V      # 宿主直接运行，输出 nginx/1.29.8

# bundle: features = []
phprt install nginx@1.29 --instance php-8.5 --force
readelf -d .phprt/nginx-1.29.8/sbin/nginx | grep NEEDED
# 回到 [libcrypt.so.1] + 其他，和关掉开关前一致
```

- 关掉 feature 时 `deps/` 里还留着上次编的 `libcrypt.a`，也不会被悄悄链进去：
  feature 关闭后 nginx 的 `--with-ld-opt` 里没有 libxcrypt 的 `-L`。
- `plan` 的构建顺序里，feature 打开时是 6 个节点（含 `dep:libxcrypt 4.5.2`），
  关闭时是 5 个。

另外用合成层验证了解包器：opaque / 普通 whiteout 先于本层条目应用、非法
whiteout 直接报错且不删父目录、符号链接越界被拒绝、文件与目录互换能重建、
硬链接和 mtime 能复制到实例、目录 mode 能恢复。
