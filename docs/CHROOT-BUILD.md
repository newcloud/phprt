# chroot 构建环境（buildroot）设计

- 状态：已实现并实测（`./phprt buildroot`，实现为 `scripts/buildroot.sh`）；项目级集成待做
- 日期：2026-11-07（2026-11-08 更新：合并为单一入口、最小 /dev、`--remove`、flock、dnf 缓存）
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
- 暂不支持非 dnf 系发行版（create 时明确报错，不假装支持）
- 不做远程/云构建

## 目录布局

当前实现（单一入口，`./phprt buildroot`）：

```
<仓库>/
  buildroots/
    <名字>/                     # dnf --installroot 目标 = chroot 根（~659M）
      usr/ bin/ etc/ ...
  .buildroot.conf               # 记录默认 rootfs 路径和绑定目录
  .buildroot.lock               # 运行期互斥锁
  scripts/buildroot.sh          # 唯一实现（create/enter/unmount/remove/status/list/use）
```

将来集成进项目时改成 `<项目>/.phprt/buildroots/<名字>/`，命令不变。

## 命令

```
./phprt buildroot create [名字] [releasever]
./phprt buildroot enter  [名字]
./phprt buildroot unmount [名字]
./phprt buildroot remove [名字] [--dry-run] [--force]
./phprt buildroot status [名字]
./phprt buildroot list
./phprt buildroot use <名字>
```

- `create`：`dnf --installroot` 装最小包清单 → 复制 resolv.conf → 写入 `.buildroot.conf`
  （默认名 `buildroot`；releasever 默认跟随系统；幂等，可重复执行续装）
- `enter`：挂载 → 进入 shell，`exit` 时自动卸载；带名字时同时设为默认
- `unmount`：手动清理异常残留的挂载（可随时执行，不加锁）
- `remove`：先卸载，确认 0 挂载后再删；`--dry-run` 只预览，`--force` 跳过进程检查
- `status`：路径、大小、包数、挂载、占用进程
- `list`：列出 `buildroots/` 下所有 rootfs
- `use`：改默认 rootfs，不进入

日常只会用到三个：`create` → `enter` → 用完 `exit`（自动卸载）。

## 挂载生命周期

| 操作 | mount | unmount |
| --- | --- | --- |
| `buildroot create` | 不需要（dnf 直接在宿主跑） | — |
| `set` / `add ext` / `update`（chroot 模式） | 构建开始前挂载 | **结束、失败、Ctrl+C 立即自动卸载** |
| `buildroot enter` | 进入时挂载 | `exit` 时卸载 |
| `buildroot status` | 不挂载 | 只显示状态 |
| `buildroot unmount` | — | 手动清理残留 |
| `buildroot remove` | — | 先卸载，再删目录 |

`buildroots/<名字>` 平时就是**未挂载状态**，只有构建/enter 期间短暂挂载。

实际挂载只有 6 处：`/proc`、`/sys`、`/dev`（tmpfs）、`/dev/pts`（独立 devpts）、
`/dev/shm`（tmpfs）、项目目录（bind）。不使用 `--rbind /dev`，宿主磁盘和输入设备
不会出现在 chroot 里。

## 构建集成

`common.sh` 新增统一包装，4 个构建入口全覆盖：

- `run_build_env()`：host 模式直接执行；chroot 模式走 `chroot_exec()`
- 接入点：`phprt set` 正式构建、`phprt add ext`、`phprt update`
  （`set --plan` 只读，不走 chroot；`--recompile` / `--clean-src` 随 set 自动覆盖）
- `chroot_exec()` 流程：
  1. rootfs 不存在 → 报错提示先跑 `phprt buildroot create`
  2. 挂载 `/proc`、`/sys`；`/dev` 用 tmpfs + 只创建 null/zero/full/random/urandom/tty
     + 独立 `devpts newinstance` + `/dev/shm`（**不 rbind 宿主 /dev**）；
     `mount --bind` 项目目录和 phprt 仓库到**同路径**（保证 configure 的 `--prefix` 等绝对路径不变）
  3. `sudo chroot --userspec=<宿主uid:gid> <rootfs>` 执行
  4. `trap EXIT INT TERM` 自动卸载（返回码透传）
- 产物属主：`--userspec` 以宿主用户构建，天然正确，不做 chown
- 并发：集成后沿用项目构建锁（`.phprt.lock`）；独立脚本自带 `flock`（`.buildroot.lock`），
  create / enter / remove 互斥，`unmount` 不加锁可随时兜底

## 启用方式

- 默认 `BUILD_ENV=host`（现状）
- `phprt set --chroot` → 写 `project.conf` 的 `BUILD_ENV=chroot`；`--host` 切回
- `add` / `update` / `--recompile` 读配置自动沿用
- `PHPRT_BUILD_ENV` 环境变量可临时覆盖

## 发行版与版本（不写死）

- 从宿主 `/etc/os-release` 读 `ID` / `ID_LIKE` / `VERSION_ID`
- 只支持 dnf 系（Fedora/RHEL/CentOS）；其它系统 create 时报错：
  「chroot 构建目前只支持 dnf 系发行版」
- `releasever` **默认 = 当前系统 VERSION_ID**，不硬编码；
  `--releasever <版本>` 可显式覆盖
- create 输出明确提示：「默认与当前系统一致；如需其它版本用 --releasever 指定」

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

- `create` 完成打印：rootfs 路径、大小、下一步命令
- `enter` 进入前后打印环境路径；退出打印「挂载已自动卸载」
- 构建结束/失败打印：「已自动卸载；如残留执行 `phprt buildroot unmount`」
- `status` 显示挂载、占用进程和剩余空间

## 参考实现（`scripts/buildroot.sh`，经 `./phprt buildroot` 调用）

```bash
cd <仓库>
./phprt buildroot create              # 创建/续装默认 rootfs（releasever 跟随系统）
./phprt buildroot create abc          # 命名 rootfs：buildroots/abc
./phprt buildroot enter abc           # 进入（exit 自动卸载，并设为默认）
./phprt buildroot unmount             # 手动兜底卸载
./phprt buildroot remove --dry-run    # 预览删除对象，不做修改
./phprt buildroot remove              # 卸载 → 确认 0 挂载 → 删除
./phprt buildroot remove abc --force  # 仍有进程占用时强制
./phprt buildroot status / list / use <名字>
```

- rootfs 统一在 `<仓库>/buildroots/<名字>`；默认名 `buildroot`
- 默认 rootfs 记录在 `.buildroot.conf`；`enter <名字>` / `use <名字>` 可切换；
  临时覆盖用 `PHPRT_CHROOT_ROOT=<路径>`
- dnf 下载缓存默认 `/var/cache/dnf-phprt`（`keepcache=True` + 共享 `cachedir`），
  删除/重建 rootfs 不丢包；实测第二次重建 54.5s → 12s（缓存 294M），可用 `PHPRT_DNF_CACHE` 覆盖
- chroot 内 `/dev` 为最小 tmpfs（6 个设备节点 + 独立 devpts + shm），不暴露宿主设备
- `trap cleanup EXIT` 在首个 mount 之前，mount 中途失败也会回滚
- `remove`：只接受 `buildroots/` 下、看起来是 rootfs 的目录；拒绝符号链接；
  卸载后残留必须为 0；删除用 `rm -rf --one-file-system`；有进程占用时默认拒绝（`--force` 跳过）
- `create` / `enter` / `remove` 用 `flock` 互斥；`unmount` 不加锁，可随时兜底
- 实测：409 包 / 659M；挂载 6 处；异常退出（kill -9）残留可用 `unmount` 或 `remove` 清理

## 与旧脚本的关系

早期的 `scripts/setup-build-chroot.sh` + 生成的 `enter-build-chroot.sh` /
`unmount-build-chroot.sh` 已合并为单一入口 `scripts/buildroot.sh`（`./phprt buildroot ...`），
旧文件已删除。

## 待确认

1. 路径语义：项目级集成时 rootfs 放到 `<项目>/.phprt/buildroots/<名字>/`
2. 集成完成后 `buildroots/` 与 `.buildroot.conf` 跟随项目目录

## 实测计划

1. 全新 `buildroot create` → `set --chroot` 全量构建 → `./myphp -v` + `phprt verify`
2. `add ext` / `update` / `--recompile` 走 chroot
3. Ctrl+C 中断 → 挂载已自动清理 → `buildroot unmount` 兜底
4. `status` / `enter` / `remove`
5. 性能 bench 与宿主构建一致（参考已有结论：差值 <0.2%，噪声范围）
