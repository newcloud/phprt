#!/usr/bin/env bash
# setup-build-chroot.sh — 用 dnf --installroot 创建能编译 phprt 的 chroot 环境
#
# 原理：dnf --installroot 把系统装到 <仓库>/buildroot/，再用 chroot + 绑定挂载
#       获得隔离的编译环境；项目父目录绑定进 chroot 后路径不变，phprt/blog 直接可用。
#
# 用法：
#   ./scripts/setup-build-chroot.sh [--remove [--dry-run] [--force]] [chroot路径] [releasever]
#     --remove    安全删除 rootfs：先卸载 → 确认 0 挂载 → 再删除（推荐，勿手写 rm -rf）
#     --dry-run   只预览删除对象（隐含 --remove），不做任何修改
#     --force     仍有进程使用 rootfs 时也继续（默认拒绝并列出 PID）
#     chroot路径  默认 <仓库>/buildroot（只允许 <仓库>/buildroot*）
#     releasever  默认与当前系统 VERSION_ID 一致（如需其它版本再指定）
#
# 生成的脚本（放在仓库根，与 buildroot/ 同级）：
#   enter-build-chroot.sh     进入环境（退出时自动卸载所有挂载）
#   unmount-build-chroot.sh   卸载全部挂载（proc/sys/dev/pts/项目绑定）
#
# 安全约定：
#   1. chroot 内 /dev 是最小化 tmpfs（只放 null/zero/full/random/urandom/tty
#      + 独立 devpts），不再 rbind 宿主 /dev，宿主磁盘/输入设备不进 chroot。
#   2. 删除必须走 --remove：卸载 → 残留必须为 0 → 才删除；rm 带 --one-file-system。
#   3. setup/enter/remove 互斥（flock），不会同时操作同一 rootfs。
#
# 环境变量：PHPRT_CHROOT_PASS  sudo 密码（默认交互输入）
#          PHPRT_CHROOT_BIND   要绑定进 chroot 的目录（默认仓库的父目录）
#
# 注：本脚本是设计文档 docs/CHROOT-BUILD.md 的参考实现；
#     将来集成进 phprt buildroot 子命令后由它取代。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOST_PROJECT="$(cd "$SCRIPT_DIR/.." && pwd)"
BIND_ROOT="${PHPRT_CHROOT_BIND:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
log()  { printf '[chroot] %s\n' "$*"; }
die()  { printf '错误: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '7,29p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

REMOVE=0
DRY=0
FORCE=0
POSARGS=()
for a in "$@"; do
  case "$a" in
    --remove)  REMOVE=1 ;;
    --dry-run) REMOVE=1; DRY=1 ;;
    --force)   FORCE=1 ;;
    -h|--help) usage; exit 0 ;;
    *)         POSARGS+=("$a") ;;
  esac
done
CHROOT="${POSARGS[0]:-$HOST_PROJECT/buildroot}"
RELVER="${POSARGS[1]:-$(. /etc/os-release 2>/dev/null && echo "${VERSION_ID:-}")}"
SUDO_PASS="${PHPRT_CHROOT_PASS:-}"
LOCK="$HOST_PROJECT/.buildroot.lock"

# 精确统计 rootfs 内挂载数（不用 grep 子串匹配，避免 buildroot-old 之类误判）
mount_count() {
  if command -v findmnt >/dev/null 2>&1; then
    findmnt -rn -o TARGET 2>/dev/null \
      | awk -v r="$CHROOT" '$0==r || index($0, r"/")==1' | wc -l
  else
    mount | grep -c -- "$CHROOT" 2>/dev/null || true
  fi
}

# 在命令替换里调用 sudo 时，sudo 的票据按父进程缓存，会取不到已有授权；
# 有密码变量时显式用 -S 喂密码，避免 dry-run 这类子 shell 静默失败。
sudo_q() {
  if [ -n "$SUDO_PASS" ]; then
    printf '%s\n' "$SUDO_PASS" | sudo -S "$@"
  else
    sudo "$@"
  fi
}

# setup / enter / remove 互斥；fd 9 由调用方决定是否传给子进程
take_lock() {
  command -v flock >/dev/null 2>&1 || return 0
  exec 9>"$LOCK"
  flock -n 9 || die "另一个 buildroot 操作正在进行（锁: $LOCK）"
}

# 校验绑定目录：必须非空、绝对路径、不是 /
check_bind() {
  [ -n "$BIND_ROOT" ] && [ "$BIND_ROOT" != "/" ] || die "绑定目录不能为空或 /（当前: '$BIND_ROOT'）"
  case "$BIND_ROOT" in
    /*) ;;
    *) die "绑定目录必须是绝对路径（当前: '$BIND_ROOT'）" ;;
  esac
}

# 列出 root（或 cwd）落在 rootfs 内的进程 PID
chroot_procs() {
  local real="$1"
  sudo_q sh -c '
    real="$1"
    for p in /proc/[0-9]*; do
      [ -d "$p" ] || continue
      r=$(readlink "$p/root" 2>/dev/null) || continue
      if [ "$r" = "$real" ]; then echo "${p#/proc/}"; continue; fi
      c=$(readlink "$p/cwd" 2>/dev/null) || c=""
      case "$c" in "$real"|"$real"/*) echo "${p#/proc/}";; esac
    done
  ' sh "$real"
}

# 安全删除：卸载 → 确认无残留 → 再删除
remove_rootfs() {
  local real
  case "$CHROOT" in
    /*) ;;
    *) die "拒绝删除：路径必须是绝对路径（当前: '$CHROOT'）" ;;
  esac
  [ ! -L "$CHROOT" ] || die "拒绝删除：$CHROOT 是符号链接"
  real="$(realpath -m -- "$CHROOT")"
  case "$real" in
    "$HOST_PROJECT"/buildroot|"$HOST_PROJECT"/buildroot-*|"$HOST_PROJECT"/buildroot.*) ;;
    *) die "拒绝删除：只允许 $HOST_PROJECT/buildroot*（当前解析为: $real）" ;;
  esac

  local scripts=("$HOST_PROJECT/enter-build-chroot.sh" "$HOST_PROJECT/unmount-build-chroot.sh")

  if [ ! -d "$real" ]; then
    log "不存在，无需删除: $real"
    local f
    for f in "${scripts[@]}"; do
      [ -e "$f" ] && rm -f -- "$f" && log "已删除生成脚本: $f"
    done
    return 0
  fi

  local procs
  procs="$(chroot_procs "$real" | tr '\n' ' ' | sed 's/ *$//')"

  if [ "$DRY" -eq 1 ]; then
    echo "== dry-run：不做任何修改 =="
    echo "  rootfs    : $real"
    echo "  大小      : $(sudo_q du -sh -- "$real" 2>/dev/null | cut -f1)"
    echo "  当前挂载  : $(mount_count) 个"
    if [ "$(mount_count)" -ne 0 ]; then
      findmnt -rn -o TARGET,SOURCE 2>/dev/null \
        | awk -v r="$real" '$1==r || index($1, r"/")==1 {printf "    %s <- %s\n", $1, $2}'
    fi
    echo "  使用进程  : ${procs:-无}"
    echo "  将删除脚本: ${scripts[*]}"
    echo "  删除方式  : 先卸载 → 确认 0 挂载 → sudo rm -rf --one-file-system"
    return 0
  fi

  if [ -n "$procs" ]; then
    if [ "$FORCE" -ne 1 ]; then
      echo "拒绝删除：仍有进程在使用 $real：" >&2
      local p
      for p in $procs; do echo "  PID $p" >&2; done
      echo "先退出/结束这些进程后重试；确认要强制删除可加 --force" >&2
      exit 1
    fi
    log "警告: 仍有进程在使用（--force 继续）: $procs"
  fi

  log "先卸载 $real 的全部挂载"
  if [ -x "$HOST_PROJECT/unmount-build-chroot.sh" ]; then
    PHPRT_CHROOT_PASS="$SUDO_PASS" "$HOST_PROJECT/unmount-build-chroot.sh" 9>&- || true
  fi
  # 兜底：仍有残留时逐个显式 lazy 卸载（子挂载在前，父挂载在后）
  if [ "$(mount_count)" -ne 0 ]; then
    sudo umount -l "$real/dev/pts"   2>/dev/null || true
    sudo umount -l "$real/dev/shm"   2>/dev/null || true
    sudo umount -l "$real$BIND_ROOT" 2>/dev/null || true
    sudo umount -l "$real/dev"       2>/dev/null || true
    sudo umount -l "$real/proc"      2>/dev/null || true
    sudo umount -l "$real/sys"       2>/dev/null || true
  fi

  local n
  n="$(mount_count)"
  if [ "$n" -ne 0 ]; then
    echo "拒绝删除：仍有 $n 个挂载残留，未做任何删除操作" >&2
    findmnt -rn -o TARGET,SOURCE 2>/dev/null \
      | awk -v r="$real" '$1==r || index($1, r"/")==1' >&2 || true
    exit 1
  fi

  log "挂载已确认清零，开始删除 $real"
  # --one-file-system：不跨文件系统，任何残留挂载都不会被递归删除
  sudo rm -rf --one-file-system -- "$real"
  log "已删除 rootfs: $real"

  local f
  for f in "${scripts[@]}"; do
    [ -e "$f" ] && rm -f -- "$f" && log "已删除生成脚本: $f"
  done
}

# 0. 前置校验 + 互斥锁 + sudo
check_bind
take_lock
if [ -n "$SUDO_PASS" ]; then
  echo "$SUDO_PASS" | sudo -S -v >/dev/null 2>&1 || die "sudo 密码错误"
else
  sudo -v
fi

# --remove / --dry-run：处理完即退出
if [ "$REMOVE" -eq 1 ]; then
  remove_rootfs
  exit 0
fi

# 1. 安装构建依赖（最小显式列表，不用 @development-tools 组：
#    该组会拖入 mesa/llvm/clang/doxygen/git-doc 等几百 MB 无关包）
#    依据：README「前置依赖」表 + runner 脚本实际调用的命令。
#    git 不装（下载全走 curl）；autoconf/re2c 不装（phprt 自动装到 tools/）。
[ "$(mount_count)" -eq 0 ] || die "rootfs 内仍有 $(mount_count) 个挂载（可能正在使用），先跑 unmount-build-chroot.sh"
log "dnf --installroot=$CHROOT 安装构建依赖（releasever 默认跟随当前系统）..."
RELVER_ARGS=()
[ -n "$RELVER" ] && RELVER_ARGS=(--releasever="$RELVER")
sudo dnf -y \
  --installroot="$CHROOT" --use-host-config "${RELVER_ARGS[@]}" \
  --setopt=install_weak_deps=False \
  install gcc gcc-c++ make cmake patchelf \
    pkgconf-pkg-config perl m4 bison re2c \
    tar xz bzip2 unzip curl binutils diffutils gawk openssl util-linux \
    >/dev/null 9>&-
# 注：perl 必须用完整包（perl-interpreter 缺 lib.pm，OpenSSL Configure 会失败）
#     util-linux 提供 flock（phprt 构建锁，need_cmds flock 会 die）
log "依赖安装完成"

# 2. chroot 内联网（resolv.conf 可能是悬空符号链接，先删再复制）
sudo rm -f "$CHROOT/etc/resolv.conf"
sudo cp -L /etc/resolv.conf "$CHROOT/etc/resolv.conf"
log "已复制 /etc/resolv.conf"

# 3. 生成进入脚本（放在仓库根，与 buildroot/ 同级）
cat > "$HOST_PROJECT/enter-build-chroot.sh" <<'EOF'
#!/usr/bin/env bash
# enter-build-chroot.sh — 进入构建 chroot；退出时自动卸载所有挂载
# /dev 是最小化 tmpfs（不 rbind 宿主 /dev），只放构建必需的设备节点
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$HERE/buildroot"
BIND="${PHPRT_CHROOT_BIND:-$(cd "$HERE/.." && pwd)}"
PASS="${PHPRT_CHROOT_PASS:-}"
LOCK="$HERE/.buildroot.lock"

die() { printf '错误: %s\n' "$*" >&2; exit 1; }

left() {
  if command -v findmnt >/dev/null 2>&1; then
    findmnt -rn -o TARGET 2>/dev/null \
      | awk -v r="$ROOT" '$0==r || index($0, r"/")==1' | wc -l
  else
    mount | grep -c -- "$ROOT" 2>/dev/null || true
  fi
}

[ -d "$ROOT" ] || die "缺少 rootfs: $ROOT（先跑 scripts/setup-build-chroot.sh）"
[ -n "$BIND" ] && [ "$BIND" != "/" ] || die "绑定目录不能为空或 /（当前: '$BIND'）"
case "$BIND" in /*) ;; *) die "绑定目录必须是绝对路径（当前: '$BIND'）" ;; esac

# 互斥：另一个 enter/remove/setup 正在跑则拒绝
if command -v flock >/dev/null 2>&1; then
  exec 9>"$LOCK"
  flock -n 9 || die "另一个 buildroot 操作正在进行（锁: $LOCK）"
fi
# 残留挂载（上次异常退出）必须先用 unmount 脚本清理
[ "$(left)" -eq 0 ] || die "rootfs 内已有 $(left) 个挂载（上次未正常退出？），先跑 unmount-build-chroot.sh"

if [ -n "$PASS" ]; then echo "$PASS" | sudo -S -v; else sudo -v; fi

cleanup() {
  sudo umount -R "$ROOT/dev/pts" 2>/dev/null || sudo umount -l "$ROOT/dev/pts" 2>/dev/null || true
  sudo umount -R "$ROOT/dev/shm" 2>/dev/null || sudo umount -l "$ROOT/dev/shm" 2>/dev/null || true
  sudo umount -R "$ROOT$BIND"    2>/dev/null || sudo umount -l "$ROOT$BIND"    2>/dev/null || true
  sudo umount -R "$ROOT/dev"     2>/dev/null || sudo umount -l "$ROOT/dev"     2>/dev/null || true
  sudo umount -R "$ROOT/proc"    2>/dev/null || sudo umount -l "$ROOT/proc"    2>/dev/null || true
  sudo umount -R "$ROOT/sys"     2>/dev/null || sudo umount -l "$ROOT/sys"     2>/dev/null || true
}
# trap 必须在第一个 mount 之前，mount 中途失败也能回滚
trap cleanup EXIT

sudo mount -t proc  proc  "$ROOT/proc"
sudo mount -t sysfs sys   "$ROOT/sys"

# 最小化 /dev：tmpfs + 独立 devpts，不暴露宿主任何真实设备
sudo mount -t tmpfs -o mode=755,size=16m tmpfs "$ROOT/dev"
sudo mkdir -p "$ROOT/dev/pts" "$ROOT/dev/shm"
sudo mount -t devpts -o newinstance,ptmxmode=0666,mode=0620 devpts "$ROOT/dev/pts"
sudo ln -sfn pts/ptmx "$ROOT/dev/ptmx"
sudo mount -t tmpfs -o mode=1777,size=256m tmpfs "$ROOT/dev/shm"
sudo mknod -m 666 "$ROOT/dev/null"    c 1 3
sudo mknod -m 666 "$ROOT/dev/zero"    c 1 5
sudo mknod -m 666 "$ROOT/dev/full"    c 1 7
sudo mknod -m 666 "$ROOT/dev/random"  c 1 8
sudo mknod -m 666 "$ROOT/dev/urandom" c 1 9
sudo mknod -m 666 "$ROOT/dev/tty"     c 5 0
sudo ln -sfn /proc/self/fd   "$ROOT/dev/fd"
sudo ln -sfn /proc/self/fd/0 "$ROOT/dev/stdin"
sudo ln -sfn /proc/self/fd/1 "$ROOT/dev/stdout"
sudo ln -sfn /proc/self/fd/2 "$ROOT/dev/stderr"

sudo mkdir -p "$ROOT$BIND"
sudo mount --bind "$BIND" "$ROOT$BIND"

echo "== 进入 chroot（exit 退出） =="
# 9>&-：不把锁 fd 传给 chroot 子进程，父进程被杀时锁能立即释放
sudo chroot "$ROOT" /bin/bash --login 9>&-
EOF
chmod +x "$HOST_PROJECT/enter-build-chroot.sh"

# 4. 生成卸载脚本（取消 proc/sys/dev/pts/项目绑定等全部挂载）
cat > "$HOST_PROJECT/unmount-build-chroot.sh" <<'EOF'
#!/usr/bin/env bash
# unmount-build-chroot.sh — 卸载 chroot 的全部挂载（proc/sys/dev/pts/项目绑定）
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$HERE/buildroot"
BIND="${PHPRT_CHROOT_BIND:-$(cd "$HERE/.." && pwd)}"
PASS="${PHPRT_CHROOT_PASS:-}"
if [ -n "$PASS" ]; then echo "$PASS" | sudo -S -v; else sudo -v; fi

left() {
  if command -v findmnt >/dev/null 2>&1; then
    findmnt -rn -o TARGET 2>/dev/null \
      | awk -v r="$ROOT" '$0==r || index($0, r"/")==1' | wc -l
  else
    mount | grep -c -- "$ROOT" 2>/dev/null || true
  fi
}
echo "卸载前挂载数: $(left)"

# 逐个显式卸载（子挂载在前，父挂载在后）；-R 失败就退回 lazy
sudo umount -R "$ROOT/dev/pts" 2>/dev/null || sudo umount -l "$ROOT/dev/pts" 2>/dev/null || true
sudo umount -R "$ROOT/dev/shm" 2>/dev/null || sudo umount -l "$ROOT/dev/shm" 2>/dev/null || true
sudo umount -R "$ROOT$BIND"    2>/dev/null || sudo umount -l "$ROOT$BIND"    2>/dev/null || true
sudo umount -R "$ROOT/dev"     2>/dev/null || sudo umount -l "$ROOT/dev"     2>/dev/null || true
sudo umount -R "$ROOT/proc"    2>/dev/null || sudo umount -l "$ROOT/proc"    2>/dev/null || true
sudo umount -R "$ROOT/sys"     2>/dev/null || sudo umount -l "$ROOT/sys"     2>/dev/null || true

echo "卸载后挂载数: $(left)"
if [ "$(left)" -eq 0 ]; then
  echo "OK: chroot 挂载已全部卸载"
else
  echo "警告: 仍有挂载残留:" >&2
  if command -v findmnt >/dev/null 2>&1; then
    findmnt -rn -o TARGET,SOURCE 2>/dev/null \
      | awk -v r="$ROOT" '$1==r || index($1, r"/")==1' >&2 || true
  else
    mount | grep "$ROOT" >&2 || true
  fi
  exit 1
fi
EOF
chmod +x "$HOST_PROJECT/unmount-build-chroot.sh"

log "完成。进入环境：$HOST_PROJECT/enter-build-chroot.sh"
log "  卸载挂载：$HOST_PROJECT/unmount-build-chroot.sh"
log "  安全删除：$HOST_PROJECT/scripts/setup-build-chroot.sh --remove（可加 --dry-run 预览）"
log "  chroot 根目录: $CHROOT"
log "  绑定目录: $BIND_ROOT（chroot 内同路径；可用 PHPRT_CHROOT_BIND 覆盖）"
