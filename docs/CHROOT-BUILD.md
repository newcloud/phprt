# chroot 构建环境（buildroot）

- 状态：standalone 与自动 `--chroot` 已实现（2026-11-09）
- 入口：`phprt buildroot ...`，实现为 `scripts/buildroot.sh`
- 默认：宿主构建；只有显式 `--chroot` 或项目配置为 chroot 时才进 rootfs
- 实测：真实 dnf rootfs + `install --chroot` + `add ext --chroot` 已通过

## 1. 目标与非目标

目标：

- 用 `dnf --installroot` 创建独立 rootfs，在固定工具链里构建 PHP 和扩展。
- rootfs 跟随项目，默认放在 `<项目>/.phprt/buildroots/`，删项目就删干净。
- 构建结束主动卸载；rootfs 平时不常驻挂载。
- 自动模式：`phprt install --chroot` / `phprt add ext ... --chroot` 直接走 chroot。

非目标：

- 不替代宿主构建。
- 不支持非 dnf 系发行版（create 时明确报错）。
- 不做远程/云构建。
- 不把 rootfs 打进项目版本树。

## 2. 目录布局

```text
<项目>/.phprt/buildroots/
├── default/          # dnf --installroot 的 rootfs
├── .config           # ROOT_DEFAULT / BUILD_ENV=chroot|host
├── .lock             # buildroot 操作互斥（flock）
└── .home/            # 绑定到 chroot 的 /home/phprt，作为构建用户 HOME
```

- 简单名字映射到 `<项目>/.phprt/buildroots/<名字>`。
- 含 `/` 或以 `.` 开头的参数按当前目录解析，允许放到项目外。
- 省略参数时用 `.config` 的 `ROOT_DEFAULT`；第一次是 `default`。
- `.phprt` 顶层因此新增 `buildroots/`，其余约束不变。

## 3. 命令

```bash
phprt buildroot create [名字|路径] [releasever]
phprt buildroot enter  [名字|路径] [--root]
phprt buildroot exec   [名字|路径] -- <命令...>
phprt buildroot unmount [名字|路径]
phprt buildroot remove [名字|路径] [--dry-run] [--force]
phprt buildroot status [名字|路径]
phprt buildroot list
phprt buildroot use <名字|路径>
phprt buildroot env [chroot|host]
```

日常流程：

```bash
phprt buildroot create
phprt install 8.5 --chroot --ext "openssl zlib curl sqlite mbstring"
phprt verify
```

- `create`：dnf 装最小包 → 复制 `/etc/resolv.conf` → 写 `.phprt-buildroot` 标记。
- `enter`：挂载后进入 shell，退出自动卸载；默认用宿主用户，`--root` 用 root。
- `exec`：挂载后执行命令并透传退出码，自动模式用它包住 `build.sh`。
- `unmount`：兜底清理异常残留，不加锁。
- `remove`：先卸载，确认 0 挂载；只删带标记且像 rootfs 的目录；`--force` 跳过进程检查。
- `status`：路径、大小、包数、挂载、占用进程。
- `use`：设置默认 rootfs。
- `env`：查看或设置 `BUILD_ENV`。

## 4. 挂载与绑定

挂载点：

- `/proc`
- `/sys`（失败只警告，构建通常不需要）
- `/dev`：tmpfs + 独立 devpts + `/dev/shm`，只创建 null/zero/full/random/urandom/tty
- 项目根、phprt 仓库
- 额外的 `BUILD_ROOT`、`PHPRT_SEED`、`DOWNLOADS`（自动构建时）
- `/home/phprt`：绑定 `.phprt/buildroots/.home`

关键约束：**bind 必须保持和宿主相同的绝对路径**。v2 的 `--prefix`、
`--with-openssl=...`、`--tree` 都写绝对路径；换路径会让 configure 和产物失效。

### rootfs 在项目内

rootfs 在 `<项目>/.phprt/buildroots/` 下，而项目根又 bind 进 chroot，所以
chroot 内通过项目路径能再看到 rootfs 一次。构建只使用精确的 `--tree` /
`--prefix` 路径，不做项目递归扫描。

不在项目 bind 上再叠 tmpfs 去隐藏 `.phprt/buildroots`：那种做法会把宿主
的 `.phprt/buildroots` 也盖住，反而让 rootfs 在宿主编译进程里消失。

## 5. 自动 chroot

```bash
phprt install 8.5 --chroot --ext "curl redis"
phprt add ext tidy --shared --chroot

phprt install 8.5 --host          # 切回宿主
```

- `--chroot` / `--host` 会写 `.phprt/buildroots/.config`，后续 `install` / `add ext` 沿用。
- `PHPRT_BUILD_ENV=chroot|host` 可临时覆盖，不写配置。
- `--plan` 只打印计划，不进 chroot。
- 自动模式只包住 `runner/build.sh`；`activate_tree`、ini、`myphp` 仍在宿主执行。
- 构建锁在宿主持有；进入 chroot 前关闭锁 fd，避免子进程继承。

### 环境变量

`buildroot.sh exec` 用 `env -i` 显式传入白名单：

```text
PHPRT_REPO PHPRT_RUNNER PHPRT_PROJECT PHPRT_HOME PHPRT_WORK
PHPRT_SEED PHPRT_DOWNLOADS PHPRT_PHP_VERSION PHPRT_EXT_MODES
PHPRT_FORCE_PHP PHPRT_CLEAN_SRC PHPRT_LINK_MODE PHPRT_LTO
BUILD_ROOT BUNDLE TREE PREFIX DEPS SELECTED_EXTS DOWNLOADS VENDOR
JOBS OFFLINE PHPRT_PROXY https_proxy HTTPS_PROXY http_proxy HTTP_PROXY
HOME=/home/phprt PATH=... TMPDIR=/tmp
```

不依赖 `sudo -E`，避免 sudoers 的 env_reset 差异。

## 6. 权限与产物属主

- 自动构建默认 `chroot --userspec=<宿主uid:gid>`，产物属主天然正确，不做 `chown`。
- 构建用户的 HOME 是 `<项目>/.phprt/buildroots/.home`，绑定到 `/home/phprt`。
- `enter` 默认也是宿主用户；需要 root shell 时加 `--root`。
- 用户命名空间/容器里如果 `--userspec` 不可用，可设
  `PHPRT_CHROOT_AS_ROOT=1`；这时产物属主由调用环境决定。
- sudo：终端交互输入；非交互可用 `PHPRT_CHROOT_PASS`。

## 7. dnf 包清单

```text
gcc gcc-c++ make cmake patchelf pkgconf-pkg-config perl m4 bison re2c
tar xz bzip2 unzip curl binutils diffutils gawk openssl util-linux
ca-certificates
```

- `--setopt=install_weak_deps=False`，避免 perl 拉入大量弱依赖。
- `perl` 必须是完整包：`perl-interpreter` 缺 `lib.pm`，OpenSSL Configure 会失败。
- `util-linux` 提供 `flock`；`binutils` 提供 `readelf`。
- dnf 缓存默认 `/var/cache/dnf-phprt`，删除/重建 rootfs 不重新下载。

## 8. 安全约定

- `remove` 只处理带 `.phprt-buildroot` 标记、看起来像 rootfs 的目录。
- 拒绝符号链接、`/`、`$HOME`。
- 删除前先卸载，确认 0 挂载；有进程占用时默认拒绝，`--force` 跳过。
- `create` / `enter` / `remove` / `exec` 用 `flock` 互斥。
- `/dev` 不 rbind 宿主，只创建最小设备节点；用户命名空间下 mknod 失败时
  退回 bind 单个宿主设备节点。

## 9. 实测

2026-11-09 真实环境（Fedora 43，sudo 非交互）：

- `create`：409 包 / 659M；dnf 缓存复用后再次 create 约 20s。
- `phprt install 8.5 --chroot --ext "openssl zlib curl sqlite mbstring"`：
  在 chroot 里完成依赖和 PHP 构建；宿主 `phprt verify` 全绿。
- `phprt add ext tidy --shared --chroot`：只编 tidy 和它的依赖，宿主功能调用通过。
- 产物属主是宿主 `1000:1000`（`--userspec`），不是 root。
- `exec` / `enter` 结束、构建成功或失败后，rootfs 下挂载数都是 0。
- 用户命名空间（`unshare -rmp`）也验证过：`sysfs` 失败只警告、mknod 退回
  bind 宿主设备节点；真实 root 下正常 mknod。
- 项目 / 仓库路径在 chroot 内保持不变；`HOME` 是绑定到 `/home/phprt`
  的 `.phprt/buildroots/.home`。

## 10. 限制

- 只支持 dnf 系发行版。
- rootfs 在项目内时，chroot 内通过项目 bind 还会看到它一次；构建只用精确路径。
- `PHPRT_WORK`、`PHPRT_SEED`、`PHPRT_DOWNLOADS` 在项目/仓库外时会自动追加 bind；
  其他外部路径需要放进 `PHPRT_CHROOT_BINDS`。
- 仓库/项目的 `downloads/` 如果是软链，会自动把真实目录也 bind 进去；
  宿主 `~/.cache/phprt/downloads` 仍不会自动进入 chroot，需要时用
  `PHPRT_SEED` 或 `PHPRT_DOWNLOADS`。
