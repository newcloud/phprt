# OCI 构建环境验证记录（youki / libcontainer）

**本文是一次性验证记录，只作历史依据，不描述现行行为；现行行为以
[BUILD-IMAGES.md](../BUILD-IMAGES.md) 为准。** 验证代码没有进仓库，只记录方法、结果、
踩到的坑和由此得到的实现要求。

> 存档说明：验证时 `recipes/images.toml` 还有 `[images.*]` 镜像线条目，后来删除；
> 文中提到的 digest / 条目只作当时的记录，现行配置只有 `[families.*]` 和
> `[package_maps.*]`。

- 验证日期：2026-11-22 ~ 2026-11-23
- 验证分支：`phprtv3-test-youki-build-image`
- 结论：**路线可行**。自拉 OCI、自己解包、libcontainer rootless 起容器、多次 exec、
  dnf 装包、容器内跑完整 `phprt install php@8.5`，产物在宿主上可直接运行。
  验证中踩到的问题和处理方式见第 7 节。

---

## 1. 验证目的

回答四个问题：

1. 不依赖 podman / docker，能不能用 Rust 的 registry 客户端 + youki 的
   `libcontainer`，从 Registry 拉一个基础镜像并起一个长期运行的 rootless 容器？
2. 同一个容器能不能多次执行独立命令，并且第二次不重装工具？
3. 容器里缺的构建工具，能不能根据现有的 `phprt plan` 输出 + `recipes/images.toml`
   的 `[package_maps.dnf]` 装进容器？
4. 现有 `phprt install` 在容器里原样跑，能不能完成完整构建，产物能不能在宿主上运行？

不验证：apt 系列镜像、多架构、overlayfs、镜像 GC、离线模式、真实 CLI 参数接入。

---

## 2. 验证环境

```text
宿主系统   Fedora Linux 43
内核       7.0.10-101.fc43.x86_64
Rust       1.96.0 / cargo 1.96.0
UID/GID    1000(z):1000(z)
SELinux    Enforcing（容器进程 unconfined_t）
init       systemd（user manager 在 /run/user/1000）
cgroup     cgroup v2
subuid     /etc/subuid: z:524288:65536
subgid     /etc/subgid: z:524288:65536
newuidmap  /usr/bin/newuidmap
newgidmap  /usr/bin/newgidmap
libseccomp 2.6.0（含 -devel）
```

镜像：

```text
quay.io/fedora/fedora:43
```

拉取当时的 digest（后面用它判断 tag 是否漂移）：

```text
index（linux/amd64 列表） sha256:ef37658952c5debb727f508f76b9fbe502ccb10f92029bc8c308560c14a79862
amd64 manifest            sha256:c9fbabbeee0eed88dbeb4ff21d29f7d38abadf07d512016ab8eb17a41c5ac167
config                    sha256:b0cfc0d0257ea873b471d1e7d9b731d94b5f70f5941c67bdc09ff278ed02264a
layer（单层 gzip）        sha256:b0cba43eac54a7a4820ed99df779cdaaf7325d768deff2f2cd78268fcd7e6b2f
                         69,851,151 字节
```

镜像 `created` 时间：`2026-11-22T07:47:40Z`。仓库里 `recipes/images.toml` 记录的 digest
是 `sha256:40e048329e89502dae22f4f35d44aaca50bffb611e5c0c9d50bbbe7a5092db9b`，
和验证拉到的三个 digest 都不一致，说明 tag 在这份记录之后被重建过。

---

## 3. 验证程序（spike）

### 3.1 位置与构建

```text
/tmp/opencode/bi-spike/
├── Cargo.toml
├── src/main.rs
├── target/debug/bi-spike
└── root/                                  # 全部验证数据
    ├── origins/oci/images/<ref>-<digest12>/
    │   ├── image.json                     # ref / digest / 层清单
    │   ├── manifest.json
    │   ├── config.json                    # 镜像 config blob
    │   ├── layers/00-sha256-<digest>      # 原始压缩层
    │   ├── rootfs/                        # 解包结果
    │   └── modes.json                     # 为复制临时改过 mode 的文件
    └── instances/<id>/bundle/
        ├── config.json                    # OCI runtime spec
        └── rootfs/                        # 实例自己的可写 rootfs
```

运行时状态：

```text
/run/user/1000/phprt-bi-spike/<id>/         # libcontainer state
/run/user/1000/phprt-bi-spike/<id>.pid
```

依赖：

```toml
anyhow = "1"
flate2 = "1"
libc = "0.2"
libcontainer = "0.7"                       # 默认 features：systemd / v2 / v1 / libseccomp
nix = { version = "0.29", features = ["process", "signal"] }
oci-client = "0.18"                        # 默认 rustls
serde / serde_json
tar = "0.4"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

环境问题：本机 cargo 默认会走 `~/.gitconfig` 里的代理 `127.0.0.1:10808`，该代理对
HTTPS 的 CONNECT 会 TLS EOF。验证时用下面的方式直连：

```bash
cargo fetch --config 'http.proxy=""'
cargo build --offline
```

### 3.2 子命令

```text
bi-spike pull <ref>              拉 manifest + config + 层，写到 origins/
bi-spike unpack <image-dir>      按层顺序解包到 rootfs/
bi-spike info <image-dir>        打印 digest、层、镜像 env / cmd
bi-spike run <id> <image-dir>    复制 rootfs 到实例，生成 config.json，创建并启动容器
bi-spike exec <id> -- <cmd...>   在容器里执行一条命令并等待退出
bi-spike status <id>             读 libcontainer state
bi-spike stop <id>               Container::delete(true)
```

### 3.3 关键实现选择

**拉镜像**

- 用 `pull_manifest_and_config_and_list_digest` 拿 manifest / config / index digest。
- **逐层 `pull_blob` 顺序下载**，不用 `Client::pull`：后者内部用
  `buffer_unordered` 收集层，顺序没有保证，分层解包会错。
- 层文件按 `layers/<序号>-<digest>` 落盘；`image.json` 记录顺序和 mediaType。
- 只实现 gzip 和未压缩 tar；遇到 zstd 直接报错。

**解包**

- 按 `image.json` 的层顺序依次应用。
- 处理 `.wh.<name>`（删除对应项）和 `.wh..wh..opq`（清空目录）。
- 拒绝绝对路径和 `..`。
- 跳过字符设备 / 块设备节点。
- 不保留 uid/gid（rootless 下所有文件归宿主用户，进容器后映射成 root）。
- 保留 mode；因为镜像里有 `--x--x--x` / `r-s--x--x` 文件（sudo 等），普通用户
  无法读取，所以在解包后补 owner-read 位并记到 `modes.json`，复制到实例后再恢复。
- 0555 目录会导致后续写入 EACCES，解包时临时给目录补 owner 权限。

**容器 spec**

- `Spec::default()` 为基线，`Linux::rootless(euid, egid)` 生成 rootless Linux 段。
- **不建 network namespace**：容器共享宿主网络，dnf 才能直接联网；本次验证不需要
  网络隔离。
- `/sys` 用宿主 `/sys` rbind 只读（rootless 下直接 mount sysfs 会失败）。
- mounts：`get_rootless_mounts()` + 以下 bind：

```text
项目根                        rw   /home/z/testphp/phprtv3-test-youki-build-image
download_dir                  rw   /home/z/testphp/downloads（在项目外，必须单独挂）
recipes                       ro
/phprt                        ro   宿主 phprt 二进制
/etc/resolv.conf              ro   镜像里没有这个文件
```

- 进程：`sleep infinity`，uid/gid 0，HOME=/root，no_new_privileges=false，
  nofile=65536。
- capabilities 用 docker 默认集：AUDIT_WRITE、CHOWN、DAC_OVERRIDE、FOWNER、FSETID、
  KILL、MKNOD、NET_BIND_SERVICE、NET_RAW、SETFCAP、SETGID、SETUID、SETPCAP、
  SYS_CHROOT。`oci-spec` 自带的默认只有 AUDIT_WRITE / KILL / NET_BIND_SERVICE，
  dnf 装包会失败。

**创建 / 启动**

```rust
ContainerBuilder::new(id, SyscallType::default())
    .with_root_path(state_dir)?
    .with_pid_file(Some(pid_file))?
    .as_init(&bundle)
    .with_systemd(true)
    .with_detach(true)
    .build()?;
container.start()?;
```

**exec（关键）**

`libcontainer 0.7` 的 `TenantContainerBuilder` 不处理 user namespace：

```text
tenant_builder.rs: NAMESPACE_TYPES = ["ipc", "uts", "net", "pid", "mnt", "cgroup", "time"]
```

直接 exec 会在 `setns(pid)` 处 EPERM。spike 的做法是：

1. 先对 `/proc/<initpid>/ns/user` 做 `setns(CLONE_NEWUSER)`；
2. 再构造 `TenantContainerBuilder`，`with_detach(false)`，`waitpid` 等退出。

`setns` 到 user namespace 要求调用进程单线程，所以 spike 的 exec 路径不启动 tokio
运行时（只有 pull 用 tokio）。

**删除**

`Container::delete(true)` 直接删，不先 `kill`。

---

## 4. 验证步骤与结果

### 4.1 拉镜像

```bash
./target/debug/bi-spike pull quay.io/fedora/fedora:43
```

```text
引用      quay.io/fedora/fedora:43
manifest  sha256:c9fbabbeee0eed88dbeb4ff21d29f7d38abadf07d512016ab8eb17a41c5ac167
index     sha256:ef37658952c5debb727f508f76b9fbe502ccb10f92029bc8c308560c14a79862
config    sha256:b0cfc0d0257ea873b471d1e7d9b731d94b5f70f5941c67bdc09ff278ed02264a (974)
layers    1
层 0 下载 sha256:b0cba43e... (69851151) ... 完成
目录      /tmp/opencode/bi-spike/root/origins/oci/images/quay.io-fedora-fedora-43-c9fbabbeee0e
```

结果：成功。匿名拉取可用；单层 70 MB；从开始到完成约半分钟。

镜像 config：

```json
"config": {
  "Env": ["PATH=/usr/local/bin:/usr/bin", "container=oci"],
  "Cmd": ["/bin/bash"],
  "WorkingDir": "/"
}
```

注意 PATH 里没有 `/sbin`、`/usr/sbin`。

### 4.2 解包

```bash
./target/debug/bi-spike unpack <image-dir>
```

```text
[0] 解包 sha256:b0cba43e... (application/vnd.oci.image.layer.v1.tar+gzip)
调整可直接复制的权限 6 项
rootfs    .../rootfs
```

结果：成功，rootfs 183 MB。检查：

```text
glibc          2.42（与 recipes/images.toml 一致）
dnf            dnf5 5.2.18.0
gcc            不存在（符合预期）
sudo           mode 4111（setuid 保留）
/etc/hosts     存在
/etc/resolv.conf  不存在
```

### 4.3 创建并启动容器

```bash
./target/debug/bi-spike run test1 <image-dir>
```

```text
复制 rootfs ... -> .../instances/test1/bundle/rootfs
恢复权限 6 项
容器 test1 已启动，pid ...
state      /run/user/1000/phprt-bi-spike/test1
bundle     /tmp/opencode/bi-spike/root/instances/test1/bundle
```

libcontainer state：

```json
{
  "id": "test1",
  "status": "running",
  "pid": 4044170,
  "bundle": "/tmp/opencode/bi-spike/root/instances/test1/bundle",
  "creator": 1000,
  "useSystemd": true
}
```

结果：成功。rootless、systemd cgroup、init 进程 `sleep infinity` 常驻
（父进程退出后由 `systemd --user` 接管）。

### 4.4 exec

```bash
./target/debug/bi-spike exec test1 -- /bin/sh -c 'id; cat /etc/os-release | head -2; dnf --version | head -1'
```

```text
已进入 user namespace（uid=0）
uid=0(root) gid=0(root) groups=0(root),65534(nobody)
NAME="Fedora Linux"
VERSION="43 (Container Image)"
dnf5 version 5.2.18.0
[exit 0]
```

挂载与路径检查：

```text
/home/z/testphp/phprtv3-test-youki-build-image          rw
/home/z/testphp/downloads                               rw
/home/z/testphp/phprtv3-test-youki-build-image/recipes  ro
/phprt                                                  ro
```

```bash
/phprt --version
# phprt 0.1.0
```

结果：成功。宿主二进制在镜像里能跑；宿主和镜像同为 Fedora 43，glibc 2.42。

### 4.5 装构建工具

按 `[package_maps.dnf]` 实际装这批：

```bash
dnf install -y gcc gcc-c++ make perl m4 pkgconf patchelf
```

```text
298 packages, Complete!
```

后来又补了：

```bash
dnf install -y cmake nasm
```

复检：

```text
gcc (GCC) 15.3.1 20260722 (Red Hat 15.3.1-1)
g++ (GCC) 15.3.1 20260722 (Red Hat 15.3.1-1)
GNU Make 4.4.1
perl -MFindBin -e1        → 成功（装完整 perl 后）
pkg-config 2.3.0
patchelf 0.18.0
cmake 3.31.11
```

结果：成功。dnf 默认源速度可用（约 1.5 MB/s），没有配置国内镜像也能完成。

### 4.6 容器内 `phprt plan`

```bash
/phprt --root /home/z/testphp/phprtv3-test-youki-build-image plan php@8.5 --json
```

结果：成功，退出码 0，输出 31 KB JSON。关键内容：

```json
"toolchain": {
  "gcc": "15.3.1",
  "gxx": "15.3.1",
  "triple": "x86_64-redhat-linux",
  "missing": [],
  "gcc_band": ">= 9",
  "cflags": "",
  "cxxflags": ""
},
"host_commands": {
  "pm": "dnf",
  "missing": ["cmake"],
  "unmapped": [],
  "install": "sudo dnf install cmake"
}
```

说明：

- 容器内的编译器和发行版事实被正确识别，`ctx["host"]` 走 rpm 分支。
- 缺包清单已经由现有 `plan` 给出，不需要再实现一套推导。
- `install` 命令带 `sudo`，容器里以 root 跑时要剥掉；也没有 `-y`，自动化执行要补。

### 4.7 容器内完整构建

第一次直接跑会停在缺 cmake：

```bash
/phprt --root ... install php@8.5
# error: 宿主缺少构建命令：cmake
# Fedora Linux 43（dnf）上执行：
#   sudo dnf install cmake
```

装好 cmake / nasm 后重跑：

```bash
/phprt --root /home/z/testphp/phprtv3-test-youki-build-image install php@8.5
```

```text
非交互环境，跳过上游补丁检查，使用本地 catalog
...
[1/9] [deps] openssl 3.5.9 ...
...
[9/9] [product] php 8.5.13 make
[9/9] [product] php 8.5.13 install
[product] php 8.5.13 ✓ 完成
编译完成: 9 个，已构建跳过 0 个
已写入 .../.phprt/php-8.5/state.json
```

结果：成功，9 个节点，从装完包到结束约 5 分钟。

宿主验证：

```bash
./myphp-8.5 -v
# PHP 8.5.13 (cli) (built: Nov 22 2026 16:41:45) (NTS)

./myphp-8.5 -m | grep -E '^(curl|openssl|pdo_sqlite|sodium|sqlite3)$'
# curl / openssl / pdo_sqlite / sodium / sqlite3

readelf -d .phprt/php-8.5.13/bin/php | grep RPATH
# RPATH: [$ORIGIN/../lib/phprt]

stat -c '%U:%G %a %n' .phprt/php-8.5/state.json .phprt/php-8.5.13/bin/php
# z:z 644 .phprt/php-8.5/state.json
# z:z 755 .phprt/php-8.5.13/bin/php
```

openssl 的 `--openssldir` 是 `/etc/pki/tls`，说明 rpm 分支生效。

试跑 `install --downloads` 也正常：

```text
源码已下载到 /home/z/testphp/downloads（未解压）
下载完成: 0 个，已存在 9 个
```

### 4.8 删除后重建

```bash
./target/debug/bi-spike stop test2
./target/debug/bi-spike run test2 <image-dir>
```

```text
复用已有 rootfs：.../instances/test2/bundle/rootfs
容器 test2 已启动
```

重建后检查：

```text
gcc 15.3.1、cmake 3.31.11 仍在
```

结果：成功。**持久化的是实例 rootfs，不是 init 进程**；模拟重启时用同一 bundle
重新 create/start 即可，工具不需要重装。

---

## 5. 数据汇总

| 项目 | 数值 |
| --- | --- |
| 镜像压缩层 | 69,851,151 字节 |
| 解包后 rootfs | 183 MB |
| 实例 rootfs 副本 | 183 MB / 实例 |
| 构建产物 `build/` | 716 MB |
| 安装树 `.phprt/` | 117 MB |
| 拉镜像 + 解包 | 约半分钟 |
| dnf 装 298 个包 | 约 1 分钟 |
| 9 节点 PHP 完整构建 | 约 5 分钟 |
| 容器内 gcc / glibc | 15.3.1 / 2.42 |
| 产物 PHP | 8.5.13 NTS |

---

## 6. 验证方法的边界

这次是“最小可行验证程序”，不是产品实现：

- 层缓存按 `ref + manifest digest 前 12 位` 命名，没有 `images.toml` 索引、
  没有按日期分目录、没有 GC。
- 只支持 gzip / 未压缩 tar，不支持 zstd。
- 设备节点直接跳过；xattr、SELinux label、file capability 没有处理。
- 只验证 Fedora 43，apt 镜像和 `[package_maps.apt]` 没跑。
- 容器共享宿主网络（没有 network namespace），没有做网络隔离。
- 没有 overlayfs，每个实例复制整份 rootfs。
- 没有实现真实 phprt 的参数接入，只是手工 exec `phprt`。
- 没有测并发项目、rootful、`add` / `remove` / `update`、离线模式、镜像 GC。

---

## 7. 发现的问题与处理

### 7.1 catalog 手写 digest 已过期

- 现象：`recipes/images.toml` 的 fedora43 digest 是 `sha256:40e048…`；实际拉到的
  amd64 manifest 是 `sha256:c9fbab…`，index 是 `sha256:ef3765…`。
- 原因：tag 被重建（镜像 created 2026-11-22T07:47Z）。
- 处理：验证程序不依赖 catalog digest，以 Registry 返回的 manifest digest 为准；
  tag 只作为查询入口。

### 7.2 libcontainer 0.7 的 tenant exec 在 rootless 下 EPERM

- 现象：

```text
failed to create container: intermediate process error EPERM: Operation not permitted
setns(18, CLONE_NEWPID) = -1 EPERM
```

- 原因：`tenant_builder.rs` 的 `NAMESPACE_TYPES` 不含 `user`，租户 spec 没有
  user namespace，先 join pid 就失败。
- 处理：exec 前先 `setns(/proc/<initpid>/ns/user, CLONE_NEWUSER)`；要求单线程，
  所以 exec 路径不能先启动 tokio 线程池。

### 7.3 `kill` 后再 `delete` 会失败

- 现象：

```text
failed to read /sys/fs/cgroup/.../youki-test2.scope: No such file or directory
```

- 原因：SIGKILL 后 systemd 已经收掉 scope，`delete()` 再去删 cgroup 就报错。
- 处理：直接 `Container::delete(true)`，不先 kill。`delete` 不删 `*.pid`，要自己清。

### 7.4 常驻 init 继承 stdout/stderr 导致调用方挂住

- 现象：`run` 的 stdout 是管道时，命令结束了但管道不 EOF，`| tail` 一直等；
  两次验证命令因此超时 120 秒。
- 原因：`sleep infinity` 继承了调用者的 fd 1/2。
- 处理：创建容器时把 init 的 stdin/stdout/stderr 重定向到 `/dev/null`。

### 7.5 容器 id 会撞 systemd scope

- 现象：容器 id 直接用作 systemd scope 名（`youki-<id>.scope`），是用户级全局名。
- 处理：instance id 必须带项目 hash，最好再带镜像 digest 短串；不能让两个项目
  各有一个 `test1`。

### 7.6 解包要保持“可复制”

- 现象：Fedora 镜像里有 6 个 mode 为 `--x--x--x` / `r-s--x--x` 的文件（sudo 等），
  普通用户解出来后无法读取，`cp -a` 失败；0555 目录也会让后续写入 EACCES。
- 处理：解包后临时补 owner-read 位，记录原始 mode，复制到实例后恢复；目录临时
  补 owner 权限。
- 结论：产品实现需要明确的解包策略（解包身份、setuid、只读目录、xattr、设备节点），
  不能假设普通 tar 解包等价于容器存储的解包。

### 7.7 镜像里没有 `/etc/resolv.conf`

- 现象：不挂载时容器内 DNS 不可用，dnf 表现为网络失败。
- 处理：把宿主 `/etc/resolv.conf` 只读 bind 进去。

### 7.8 镜像的 PATH 不含 `/sbin` / `/usr/sbin`

- 现象：镜像 config 的 Env 是 `PATH=/usr/local/bin:/usr/bin`。
- 处理：exec 和容器内 phprt 显式设置 PATH：
  `/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin`。

### 7.9 `plan` 的装包命令带 `sudo`、没有 `-y`

- 现象：

```json
"install": "sudo dnf install cmake"
```

- 处理：容器里已经是 root，剥掉 `sudo`；自动化执行要补 `-y`。
- 结论：驱动层可以直接复用 `plan --json` 的 `host_commands.missing` / `install`，
  只需要做命令变换；或者给 plan JSON 补一个包名数组，避免解析命令字符串。

### 7.10 `phprt list` 会把 buildimages state 当成产品 state

- 实测在 `.phprt/buildimages/state.json` 放 `{"active_instance":"x"}`：

```text
product        version      tree
@                           ./.phprt/-
php@8.5        8.5.13       ./.phprt/php-8.5.13
```

- 原因：`list` 扫 `.phprt/*/state.json` 并反序列化，缺字段时全部走默认值。
- 结论：项目级状态文件不要放在会被 `*/*/state.json` 扫到的位置，或让 `list`
  排除该目录。

### 7.11 构建签名里没有镜像/编译器

- 代码位置：`src/build.rs:526` 的 `signature()`。
- 签名输入只有 `id / version / url / sha256`、工具名 + provider、命令、环境、产物、
  依赖；host 工具只记名字不记版本，也没有 toolchain / 镜像 digest。
- 结论：换镜像后可能直接复用旧安装树。实现构建环境时必须把“镜像 digest + 宿主/容器
  方式”加进签名，否则会拿到另一套 libc 编出来的产物而不重编。

### 7.12 实例目录放进项目会“自己挂自己”

- 参考做法的 `instances/<id>/bundle/rootfs` 在项目里，同时又挂载项目根；
  容器里能看到自己的 rootfs 副本，`rm -rf .phprt` 会伤到运行中的容器。
- spike 的处理：origin 和 instance 都放项目外（`/tmp/opencode/bi-spike/root`），
  bind 集合只包含项目根、downloads、recipes、`/phprt`、resolv.conf。
- 结论：实例数据的存放位置需要单独定；项目里只放轻量状态更安全。

---

## 8. 由验证得到的实现要求

1. **容器运行时**：`libcontainer` 负责 create/start/delete；exec 不要直接用
   tenant builder，先进入 user namespace，并且从单线程进程/helper 里做。
2. **stdin/stdout/stderr**：常驻 init 必须重定向到 `/dev/null`。
3. **删除**：`Container::delete(true)`；清 pid 文件和遗留 systemd failed unit。
4. **实例标识**：`<项目 hash>-<镜像 digest 短串>-<序号>`；避免 systemd scope 撞名。
5. **镜像缓存**：以 manifest digest 为内容标识；ref 只作查询入口；tag 漂移是正常现象。
6. **解包**：明确支持范围；至少处理白洞、不可读 mode、只读目录、设备节点；层必须按
   顺序应用。
7. **挂载**：从配置推导，不能假设下载目录和 recipes 在项目根内；resolv.conf 要单独挂。
8. **装包**：容器内先 `plan --json`，按 `host_commands.missing` 装包，再复检；
   剥 `sudo`、补 `-y`；apt 还要先 `update`。
9. **执行模型**：“整条命令转发进容器”已被验证可行；容器内的 `ctx["host"]`、路径、
   属主都自然正确，不需要宿主/容器分段。
10. **签名**：镜像 digest 和“宿主/容器”必须进构建签名；容器 id 不进。
11. **持久化**：持久化的是实例 rootfs；重启后从同一 bundle 重新 create/start。
12. **状态文件位置**：不能被 `phprt list` 当成产品 state；实例数据不要放在被挂载的
    项目根里。

---

## 9. 复现命令

```bash
# 准备
mkdir -p /tmp/opencode/bi-spike && cd /tmp/opencode/bi-spike
# 放入本文第 3.1 节的 Cargo.toml 和 src/main.rs
cargo fetch --config 'http.proxy=""'
cargo build --offline

# 拉镜像 + 解包
./target/debug/bi-spike pull quay.io/fedora/fedora:43
IMG=$(ls -d root/origins/oci/images/*)
./target/debug/bi-spike unpack "$IMG"
./target/debug/bi-spike info "$IMG"

# 建容器 + 验证
./target/debug/bi-spike run test1 "$IMG"
./target/debug/bi-spike exec test1 -- /bin/sh -c 'id; dnf --version | head -1'
./target/debug/bi-spike exec test1 -- dnf install -y gcc gcc-c++ make perl m4 pkgconf patchelf
./target/debug/bi-spike exec test1 -- /bin/sh -c 'gcc --version | head -1; perl -MFindBin -e1'

# 容器内 phprt
./target/debug/bi-spike exec test1 -- /bin/sh -c \
  'cd /home/z/testphp/phprtv3-test-youki-build-image && /phprt --root . plan php@8.5 --json'
./target/debug/bi-spike exec test1 -- /bin/sh -c \
  'cd /home/z/testphp/phprtv3-test-youki-build-image && /phprt --root . install php@8.5'

# 宿主验收
cd /home/z/testphp/phprtv3-test-youki-build-image
./myphp-8.5 -v
./myphp-8.5 -m | grep -E 'curl|openssl|pdo_sqlite'

# 删除
./target/debug/bi-spike stop test1
```

注意：`run` 的输出要重定向到文件，或者确保 init 的 stdio 已被重定向；否则常驻
`sleep infinity` 会持有调用者的管道。
