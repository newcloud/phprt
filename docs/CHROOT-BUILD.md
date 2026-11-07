# chroot 构建环境（buildroot）设计

- 状态：参考脚本已实现并实测（`scripts/setup-build-chroot.sh`）；`phprt buildroot` 子命令待集成
- 日期：2026-11-07（2026-11-08 更新：最小 /dev、`--remove`/`--dry-run`/`--force`、flock）
- 适用：phprt 的构建流程（`set` / `add ext` / `update`）

## 背景

phprt 现在只在宿主直接源码编译。干净环境复现、隔离宿主工具链差异等场景需要
一种可选的 chroot 构建方式：用 `dnf --installroot` 创建独立 rootfs，在里面完成
`.phprt` 的构建。该方式是**可选的**，默认仍是宿主直接编译。

## 目标

- 可选构建方式：默认宿主模式不变；chroot 模式显式启用
- 用户透明：生成的脚本和 rootfs 都在项目 `.phprt` 下，随项目删除
- 构建结束**主动 unmount**，不常驻挂载；之后使用时再 mount
- 发行版不写死：rootfs 版本默认与当前系统一致，需要时由用户指定

## 非目标

- 不替代宿主模式
- 暂不支持非 dnf 系发行版（setup 时明确报错，不假装支持）
- 不做远程/云构建

## 目录布局

```
<项目>/.phprt/
  buildroot/                    # dnf --installroot 目标 = chroot 根（~768M）
    usr/ bin/ etc/ ...
  bin/enter-buildroot.sh        # 生成：进入环境（mount + chroot + trap 卸载）
  bin/unmount-buildroot.sh      # 生成：取消 proc/sys/dev/项目绑定
  buildroot.conf                # 生成：DISTRO= / RELEASEVER= / CREATED_AT=
  project.conf                  # 增加 BUILD_ENV=chroot（--chroot 时写入）
```

## 命令

```
phprt buildroot setup [--releasever <版本>] [--yes]
phprt buildroot status
phprt buildroot enter
phprt buildroot unmount
phprt buildroot remove [--yes]
```

- `setup`：检测宿主发行版 → `dnf --installroot` 装最小包清单 → 复制 resolv.conf
  → 生成 `enter-buildroot.sh` / `unmount-buildroot.sh` / `buildroot.conf`
- `status`：路径、大小（du）、包数（rpm -qa）、发行版/版本、**当前是否挂载**（正常应为未挂载）
- `enter`：进入交互 shell（调试用），退出时卸载
- `unmount`：手动清理异常残留的挂载
- `remove`：先卸载，再删 rootfs + 生成的脚本 + buildroot.conf

## 挂载生命周期

| 操作 | mount | unmount |
| --- | --- | --- |
| `buildroot setup` | 不需要（dnf 直接在宿主跑） | — |
| `set` / `add ext` / `update`（chroot 模式） | 构建开始前挂载 | **结束、失败、Ctrl+C 立即自动卸载** |
| `buildroot enter` | 进入时挂载 | `exit` 时卸载 |
| `buildroot status` | 不挂载 | 只显示状态 |
| `buildroot unmount` | — | 手动清理残留 |
| `buildroot remove` | — | 先卸载，再删目录 |

`.phprt/buildroot` 平时就是**未挂载状态**，只有构建/enter 期间短暂挂载。

实际挂载只有 6 处：`/proc`、`/sys`、`/dev`（tmpfs）、`/dev/pts`（独立 devpts）、
`/dev/shm`（tmpfs）、项目目录（bind）。不使用 `--rbind /dev`，宿主磁盘和输入设备
不会出现在 chroot 里。

## 构建集成

`common.sh` 新增统一包装，4 个构建入口全覆盖：

- `run_build_env()`：host 模式直接执行；chroot 模式走 `chroot_exec()`
- 接入点：`phprt set` 正式构建、`phprt add ext`、`phprt update`
  （`set --plan` 只读，不走 chroot；`--recompile` / `--clean-src` 随 set 自动覆盖）
- `chroot_exec()` 流程：
  1. rootfs 不存在 → 报错提示先跑 `phprt buildroot setup`
  2. 挂载 `/proc`、`/sys`；`/dev` 用 tmpfs + 只创建 null/zero/full/random/urandom/tty
     + 独立 `devpts newinstance` + `/dev/shm`（**不 rbind 宿主 /dev**）；
     `mount --bind` 项目目录和 phprt 仓库到**同路径**（保证 configure 的 `--prefix` 等绝对路径不变）
  3. `sudo chroot --userspec=<宿主uid:gid> <rootfs>` 执行
  4. `trap EXIT INT TERM` 自动卸载（返回码透传）
- 产物属主：`--userspec` 以宿主用户构建，天然正确，不做 chown
- 并发：集成后沿用项目构建锁（`.phprt.lock`）；独立脚本自带 `flock`（`.buildroot.lock`），
  setup / enter / remove 互斥，`unmount` 不加锁可随时兜底

## 启用方式

- 默认 `BUILD_ENV=host`（现状）
- `phprt set --chroot` → 写 `project.conf` 的 `BUILD_ENV=chroot`；`--host` 切回
- `add` / `update` / `--recompile` 读配置自动沿用
- `PHPRT_BUILD_ENV` 环境变量可临时覆盖

## 发行版与版本（不写死）

- 从宿主 `/etc/os-release` 读 `ID` / `ID_LIKE` / `VERSION_ID`
- 只支持 dnf 系（Fedora/RHEL/CentOS）；其它系统 setup 时报错：
  「chroot 构建目前只支持 dnf 系发行版」
- `releasever` **默认 = 当前系统 VERSION_ID**，不硬编码；
  `--releasever <版本>` 可显式覆盖
- setup 输出明确提示：「默认与当前系统一致；如需其它版本用 --releasever 指定」

## 构建身份与权限

- `chroot --userspec=<宿主uid:gid>`：产物属主天然正确
- sudo：终端交互输入；非交互可用 `PHPRT_SUDO_PASS`
- 网络：chroot 与宿主同网络命名空间，`PHPRT_PROXY` 照常可用

## 最小包清单（实测）

```
gcc gcc-c++ make cmake patchelf pkgconf-pkg-config perl m4 bison re2c
tar xz bzip2 unzip curl binutils diffutils gawk openssl util-linux
```

- `dnf --setopt=install_weak_deps=False`（perl 会拖 130+ 个弱依赖子包）
- `perl` 必须用完整包：`perl-interpreter` 缺 `lib.pm`，OpenSSL Configure 会失败
- `util-linux` 提供 `flock`（构建锁，`need_cmds flock` 会 die）
- `git` 不需要（下载全走 curl）；`autoconf` / `re2c` 不需要（phprt 自动装到 tools/）

## 透明性提示

- setup 完成打印：环境路径、两个脚本位置、
  「卸载: phprt buildroot unmount」「删除环境: phprt buildroot remove」
- 每次 chroot 构建开头打印环境路径
- 构建结束/失败打印：「已自动卸载；如残留执行 phprt buildroot unmount」
- `status` 显示剩余挂载和删除命令

## 参考实现（`scripts/setup-build-chroot.sh`）

`phprt buildroot` 子命令集成前，用仓库内脚本验证同一套机制：

```bash
cd <仓库>
sudo ./scripts/setup-build-chroot.sh                  # 创建/续装 rootfs（releasever 默认跟随系统）
./enter-build-chroot.sh                               # 进入（exit 自动卸载）
./unmount-build-chroot.sh                             # 手动兜底卸载
sudo ./scripts/setup-build-chroot.sh --dry-run        # 预览删除对象，不做修改
sudo ./scripts/setup-build-chroot.sh --remove         # 卸载 → 确认 0 挂载 → 删除
sudo ./scripts/setup-build-chroot.sh --remove --force # 仍有进程占用时强制
```

- 生成脚本放在仓库根（与 `buildroot/` 同级）；rootfs 默认 `<仓库>/buildroot`
- dnf 下载缓存默认 `/var/cache/dnf-phprt`（`keepcache=True` + 共享 `cachedir`），
  删除/重建 rootfs 不丢包；实测第二次重建 54.5s → 12s（缓存 294M），可用 `PHPRT_DNF_CACHE` 覆盖
- chroot 内 `/dev` 为最小 tmpfs（6 个设备节点 + 独立 devpts + shm），不暴露宿主设备
- `trap cleanup EXIT` 在首个 mount 之前，mount 中途失败也会回滚
- `--remove`：只接受 `<仓库>/buildroot*`、拒绝符号链接；卸载后残留必须为 0；
  删除用 `rm -rf --one-file-system`；有进程的 root/cwd 落在该 rootfs 内时默认拒绝（`--force` 跳过）
- `setup` / `enter` / `remove` 用 `flock` 互斥；`--dry-run` 只打印预览
- 实测：409 包 / 768M；挂载 6 处；异常退出（kill -9）残留可用 `unmount` 或 `--remove` 清理

集成后 `phprt buildroot` 直接复用上述逻辑，独立脚本删除。

## 与现有独立脚本的关系

参考实现位于仓库 `scripts/setup-build-chroot.sh`（不在项目 `.phprt` 下）；生成的
`enter-build-chroot.sh` / `unmount-build-chroot.sh` 放在仓库根。集成后逻辑全部收进
`phprt buildroot`，这三个脚本与 `buildroot/` 目录删除。

## 待确认

1. 路径语义：「`./phprt/buildroot`」指 `<项目>/.phprt/buildroot/`（本设计按此实现）
2. 集成完成后删除仓库内参考脚本和生成的脚本

## 实测计划

1. 全新 `buildroot setup` → `set --chroot` 全量构建 → `./myphp -v` + `phprt verify`
2. `add ext` / `update` / `--recompile` 走 chroot
3. Ctrl+C 中断 → 挂载已自动清理 → `buildroot unmount` 兜底
4. `status` / `enter` / `remove`
5. 性能 bench 与宿主构建一致（参考已有结论：差值 <0.2%，噪声范围）
