#!/usr/bin/env bash
# buildroot.sh — chroot 构建环境的唯一入口（由 `phprt buildroot` 转发）
#
# 用法：
#   ./phprt buildroot create <路径> [releasever]   创建/续装
#   ./phprt buildroot enter  [路径]                进入（exit 自动卸载）
#   ./phprt buildroot unmount [路径]               只卸载挂载
#   ./phprt buildroot remove [路径] [--dry-run] [--force]  安全删除
#   ./phprt buildroot status [路径]                 查看状态
#   ./phprt buildroot use <路径>                    设为默认
#
# 路径规则：
#   - 相对路径按「执行命令时的当前目录」解析，如 cd blog && create ../buildrootabc
#   - 绝对路径原样使用
#   - 省略路径时用上次使用的路径（记录在仓库 .buildroot.conf）
#
# 安全约定：
#   1. chroot 内 /dev 是最小 tmpfs（null/zero/full/random/urandom/tty + 独立 devpts），
#      不 rbind 宿主 /dev。
#   2. remove 只处理参数里给的路径：必须是 create 创建的目录（有 .phprt-buildroot 标记）、
#      不是符号链接、不是 / 或 $HOME；卸载 → 残留必须为 0 → rm -rf --one-file-system。
#      有进程占用时默认拒绝（--force 跳过）。
#   3. create/enter/remove 用 flock 互斥。
#
# 环境变量：
#   PHPRT_CHROOT_PASS   sudo 密码（默认交互输入）
#   PHPRT_CHROOT_BIND   绑定进 chroot 的目录（默认仓库的父目录）
#   PHPRT_DNF_CACHE     dnf 下载缓存（默认 /var/cache/dnf-phprt）
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOST_PROJECT="$(cd "$SCRIPT_DIR/.." && pwd)"
CONF="$HOST_PROJECT/.buildroot.conf"
LOCK="$HOST_PROJECT/.buildroot.lock"
MARKER=".phprt-buildroot"

BIND_ROOT="${PHPRT_CHROOT_BIND:-$(cd "$SCRIPT_DIR/../.." && pwd)}"
SUDO_PASS="${PHPRT_CHROOT_PASS:-}"
DNF_CACHE="${PHPRT_DNF_CACHE:-/var/cache/dnf-phprt}"
RELVER_DEFAULT="$(. /etc/os-release 2>/dev/null && echo "${VERSION_ID:-}")"

log() { printf '[buildroot] %s\n' "$*"; }
die() { printf '错误: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '3,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

sudo_q() {
  if [ -n "$SUDO_PASS" ]; then
    printf '%s\n' "$SUDO_PASS" | sudo -S -p '' "$@"
  else
    sudo "$@"
  fi
}

sudo_auth() {
  if [ -n "$SUDO_PASS" ]; then
    printf '%s\n' "$SUDO_PASS" | sudo -S -p '' -v >/dev/null 2>&1 || die "sudo 密码错误"
  else
    sudo -v
  fi
}

check_bind() {
  [ -n "$BIND_ROOT" ] && [ "$BIND_ROOT" != "/" ] || die "绑定目录不能为空或 /"
  case "$BIND_ROOT" in /*) ;; *) die "绑定目录必须是绝对路径: $BIND_ROOT" ;; esac
}

take_lock() {
  command -v flock >/dev/null 2>&1 || return 0
  exec 9>"$LOCK"
  flock -n 9 || die "另一个 buildroot 操作正在进行（锁: $LOCK）"
}

load_conf() {
  ROOT_DEFAULT=""
  BIND_DEFAULT=""
  if [ -f "$CONF" ]; then
    # shellcheck disable=SC1090
    . "$CONF"
  fi
  return 0
}

write_conf() {
  local root="$1"
  printf '# 由 phprt buildroot 生成（记录上次使用的路径）\nROOT_DEFAULT=%q\nBIND_DEFAULT=%q\n' \
    "$root" "$BIND_ROOT" > "$CONF"
  log "默认路径: $root"
}


# 解析路径：相对当前目录；省略则用上次使用
resolve_path() {
  local arg="${1:-}"
  if [ -z "$arg" ]; then
    if [ -n "${PHPRT_CHROOT_ROOT:-}" ]; then
      realpath -m -- "$PHPRT_CHROOT_ROOT"
      return
    fi
    load_conf
    [ -n "${ROOT_DEFAULT:-}" ] \
      || die "请指定路径，例：./phprt buildroot create ../buildrootabc"
    printf '%s' "$ROOT_DEFAULT"
    return
  fi
  realpath -m -- "$arg"
}

looks_rootfs() {
  [ -f "$1/etc/os-release" ] && [ -x "$1/usr/bin/bash" ]
}

is_created() {
  [ -f "$1/$MARKER" ]
}

# create 时禁止直接写进仓库自身或它的源码目录
check_create_target() {
  local root="$1"
  [ "$root" != "/" ] || die "拒绝使用 / 作为 rootfs"
  [ "$root" != "$HOME" ] || die "拒绝使用 \$HOME 作为 rootfs"
  case "$root" in
    "$HOST_PROJECT"|"$HOST_PROJECT"/scripts|"$HOST_PROJECT"/runner|"$HOST_PROJECT"/bundles|\
    "$HOST_PROJECT"/docs|"$HOST_PROJECT"/downloads|"$HOST_PROJECT"/bin|"$HOST_PROJECT"/tools|\
    "$HOST_PROJECT"/vendor|"$HOST_PROJECT"/.git|"$HOST_PROJECT"/.phprt)
      die "拒绝写入仓库目录: $root" ;;
  esac
}

# remove 时只允许删除 create 创建、且确实是 rootfs 的目录
check_remove_target() {
  local root="$1"
  [ ! -L "$root" ] || die "拒绝删除：$root 是符号链接"
  [ "$root" != "/" ] || die "拒绝删除 /"
  [ "$root" != "$HOME" ] || die "拒绝删除 \$HOME"
  [ -d "$root" ] || return 0
  is_created "$root" || die "拒绝删除：$root 不是 phprt buildroot create 创建的（缺少 $MARKER）"
  looks_rootfs "$root" || die "拒绝删除：$root 不像 rootfs"
}

mount_count() {
  local root="$1"
  if command -v findmnt >/dev/null 2>&1; then
    findmnt -rn -o TARGET 2>/dev/null \
      | awk -v r="$root" '$0==r || index($0, r"/")==1' | wc -l
  else
    mount | grep -c -- "$root" 2>/dev/null || true
  fi
}

show_mounts() {
  local root="$1"
  if command -v findmnt >/dev/null 2>&1; then
    findmnt -rn -o TARGET,SOURCE 2>/dev/null \
      | awk -v r="$root" '$1==r || index($1, r"/")==1 {printf "    %s <- %s\n", $1, $2}'
  else
    mount | grep -- "$root" || true
  fi
}

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

# 逐个显式卸载（子挂载在前，父挂载在后）；-R 失败退回 lazy
unequip() {
  local r="$1"
  sudo_q umount -R "$r/dev/pts" 2>/dev/null || sudo_q umount -l "$r/dev/pts" 2>/dev/null || true
  sudo_q umount -R "$r/dev/shm" 2>/dev/null || sudo_q umount -l "$r/dev/shm" 2>/dev/null || true
  sudo umount -R "$r$BIND_ROOT" 2>/dev/null || sudo umount -l "$r$BIND_ROOT" 2>/dev/null || true
  sudo_q umount -R "$r/dev"     2>/dev/null || sudo_q umount -l "$r/dev"     2>/dev/null || true
  sudo_q umount -R "$r/proc"    2>/dev/null || sudo_q umount -l "$r/proc"    2>/dev/null || true
  sudo_q umount -R "$r/sys"     2>/dev/null || sudo_q umount -l "$r/sys"     2>/dev/null || true
}

# EXIT trap 用全局变量，避免 local 变量在脚本退出时已失效
CLEANUP_ROOT=""
cleanup() {
  [ -n "${CLEANUP_ROOT:-}" ] || return 0
  unequip "$CLEANUP_ROOT" || true
}

# ---------- create ----------
cmd_create() {
  local root relver
  root="$(resolve_path "${1:-}")"
  relver="${2:-$RELVER_DEFAULT}"
  check_bind
  check_create_target "$root"

  if [ -e "$root" ] && [ ! -d "$root" ]; then
    die "已存在同名文件，不能作为 rootfs: $root"
  fi
  if [ -d "$root" ] && [ -n "$(ls -A "$root" 2>/dev/null)" ] && ! looks_rootfs "$root"; then
    die "目录已存在且不像 rootfs，拒绝写入: $root"
  fi

  take_lock
  sudo_auth
  [ "$(mount_count "$root")" -eq 0 ] || die "rootfs 内仍有挂载，先跑：phprt buildroot unmount $root"

  sudo_q mkdir -p -- "$root"
  log "dnf --installroot=$root（releasever=${relver:-跟随系统}）"
  log "  dnf 下载缓存: $DNF_CACHE"
  sudo_q mkdir -p -- "$DNF_CACHE"
  local relargs=()
  [ -n "$relver" ] && relargs=(--releasever="$relver")
  sudo dnf -y \
    --installroot="$root" --use-host-config "${relargs[@]}" \
    --setopt=install_weak_deps=False \
    --setopt=keepcache=True \
    --setopt=cachedir="$DNF_CACHE" \
    --setopt=max_parallel_downloads=10 \
    install gcc gcc-c++ make cmake patchelf \
      pkgconf-pkg-config perl m4 bison re2c \
      tar xz bzip2 unzip curl binutils diffutils gawk openssl util-linux jq \
      >/dev/null 9>&-
  log "依赖安装完成（缓存: $(sudo_q du -sh -- "$DNF_CACHE" 2>/dev/null | cut -f1)）"

  sudo_q rm -f "$root/etc/resolv.conf"
  sudo_q cp -L /etc/resolv.conf "$root/etc/resolv.conf"

  # 标记：只有带标记的目录才允许 remove
  sudo_q sh -c 'printf "created_at=%s\n" "$1" > "$2"' sh "$(date -Is)" "$root/$MARKER"
  write_conf "$root"
  log "完成：$root（$(sudo_q du -sh -- "$root" 2>/dev/null | cut -f1)）"
  log "下一步：phprt buildroot enter $root"
}

# ---------- enter ----------
cmd_enter() {
  local root; root="$(resolve_path "${1:-}")"
  check_bind
  [ -d "$root" ] || die "rootfs 不存在: $root（先跑：phprt buildroot create $root）"
  looks_rootfs "$root" || die "目录不像 rootfs: $root"

  take_lock
  write_conf "$root"
  sudo_auth
  [ "$(mount_count "$root")" -eq 0 ] \
    || die "rootfs 内已有 $(mount_count "$root") 个挂载（上次未正常退出？），先跑：phprt buildroot unmount $root"

  CLEANUP_ROOT="$root"
  trap cleanup EXIT

  sudo mount -t proc  proc  "$root/proc"
  sudo mount -t sysfs sys   "$root/sys"

  # 最小化 /dev：tmpfs + 独立 devpts，不暴露宿主设备
  sudo mount -t tmpfs -o mode=755,size=16m tmpfs "$root/dev"
  sudo mkdir -p "$root/dev/pts" "$root/dev/shm"
  sudo mount -t devpts -o newinstance,ptmxmode=0666,mode=0620 devpts "$root/dev/pts"
  sudo ln -sfn pts/ptmx "$root/dev/ptmx"
  sudo mount -t tmpfs -o mode=1777,size=256m tmpfs "$root/dev/shm"
  sudo mknod -m 666 "$root/dev/null"    c 1 3
  sudo mknod -m 666 "$root/dev/zero"    c 1 5
  sudo mknod -m 666 "$root/dev/full"    c 1 7
  sudo mknod -m 666 "$root/dev/random"  c 1 8
  sudo mknod -m 666 "$root/dev/urandom" c 1 9
  sudo mknod -m 666 "$root/dev/tty"     c 5 0
  sudo ln -sfn /proc/self/fd   "$root/dev/fd"
  sudo ln -sfn /proc/self/fd/0 "$root/dev/stdin"
  sudo ln -sfn /proc/self/fd/1 "$root/dev/stdout"
  sudo ln -sfn /proc/self/fd/2 "$root/dev/stderr"

  sudo mkdir -p "$root$BIND_ROOT"
  sudo mount --bind "$BIND_ROOT" "$root$BIND_ROOT"

  log "进入 $root（exit 退出，自动卸载）"
  # 9>&-：锁 fd 不传给 chroot，父进程被杀时锁立即释放
  sudo chroot "$root" /bin/bash --login 9>&-
  unequip "$root"
  CLEANUP_ROOT=""
  local n; n="$(mount_count "$root")"
  if [ "$n" -eq 0 ]; then
    log "已退出，挂载已全部卸载"
  else
    log "警告: 仍有 $n 个挂载残留，执行：phprt buildroot unmount $root"
    show_mounts "$root" >&2
  fi
}

# ---------- unmount ----------
cmd_unmount() {
  local root; root="$(resolve_path "${1:-}")"
  [ -d "$root" ] || die "路径不存在: $root"
  sudo_auth
  log "卸载前挂载数: $(mount_count "$root")"
  unequip "$root"
  local n; n="$(mount_count "$root")"
  log "卸载后挂载数: $n"
  if [ "$n" -eq 0 ]; then
    log "OK: 已全部卸载"
  else
    show_mounts "$root" >&2
    die "仍有挂载残留"
  fi
}

# ---------- remove ----------
cmd_remove() {
  local path="" dry=0 force=0 a
  for a in "$@"; do
    case "$a" in
      --dry-run) dry=1 ;;
      --force)   force=1 ;;
      -*)        die "未知参数: $a" ;;
      *)         [ -z "$path" ] || die "多余参数: $a"; path="$a" ;;
    esac
  done

  local root; root="$(resolve_path "$path")"
  check_remove_target "$root"

  if [ ! -d "$root" ]; then
    log "不存在，无需删除: $root"
    load_conf
    [ "${ROOT_DEFAULT:-}" = "$root" ] && rm -f -- "$CONF"
    return 0
  fi

  sudo_auth
  local procs; procs="$(chroot_procs "$root" | tr '\n' ' ' | sed 's/ *$//')"

  if [ "$dry" -eq 1 ]; then
    echo "== dry-run：不做任何修改 =="
    echo "  rootfs    : $root"
    echo "  大小      : $(sudo_q du -sh -- "$root" 2>/dev/null | cut -f1)"
    echo "  当前挂载  : $(mount_count "$root") 个"
    show_mounts "$root"
    echo "  使用进程  : ${procs:-无}"
    echo "  删除方式  : 卸载 → 确认 0 挂载 → sudo rm -rf --one-file-system"
    return 0
  fi

  if [ -n "$procs" ]; then
    if [ "$force" -ne 1 ]; then
      echo "拒绝删除：仍有进程在使用 $root：" >&2
      local p
      for p in $procs; do echo "  PID $p" >&2; done
      echo "退出/结束这些进程后重试；确认强制删除可加 --force" >&2
      exit 1
    fi
    log "警告: 仍有进程占用（--force 继续）: $procs"
  fi

  take_lock
  log "先卸载 $root"
  unequip "$root"
  local n; n="$(mount_count "$root")"
  if [ "$n" -ne 0 ]; then
    show_mounts "$root" >&2
    die "仍有 $n 个挂载残留，未做任何删除"
  fi
  log "挂载已清零，删除 $root"
  sudo_q rm -rf --one-file-system -- "$root"
  log "已删除: $root"

  load_conf
  if [ "${ROOT_DEFAULT:-}" = "$root" ] && [ -e "$CONF" ]; then
    rm -f -- "$CONF"
    log "已清除默认路径记录: $CONF"
  fi
}

# ---------- status / list / use ----------
cmd_status() {
  local root; root="$(resolve_path "${1:-}")"
  load_conf
  local cur="${ROOT_DEFAULT:-}"
  echo "rootfs    : $root"
  if [ -n "$cur" ] && [ "$root" = "$cur" ]; then echo "默认      : 是"; else echo "默认      : 否${cur:+（当前默认 $cur）}"; fi
  if [ ! -d "$root" ]; then echo "状态      : 不存在"; return 0; fi
  if looks_rootfs "$root"; then
    if is_created "$root"; then echo "状态      : 可用"; else echo "状态      : 可用（不是 create 创建的）"; fi
  else
    echo "状态      : 目录存在但不像 rootfs"
    return 0
  fi
  sudo_auth
  echo "大小      : $(sudo_q du -sh -- "$root" 2>/dev/null | cut -f1)"
  echo "包数      : $(sudo_q rpm --root "$root" -qa 2>/dev/null | wc -l)"
  echo "挂载      : $(mount_count "$root") 个"
  show_mounts "$root"
  local p
  p="$(chroot_procs "$root" | tr '\n' ' ' | sed 's/ *$//')"
  echo "使用进程  : ${p:-无}"
}


cmd_use() {
  local root; root="$(resolve_path "${1:-}")"
  [ -d "$root" ] || die "路径不存在: $root"
  looks_rootfs "$root" || die "目录不像 rootfs: $root"
  write_conf "$root"
}

# ---------- dispatch ----------
cmd="${1:-help}"
[ $# -gt 0 ] && shift
case "$cmd" in
  create)  [ $# -le 2 ] || die "用法: phprt buildroot create <路径> [releasever]"; cmd_create "$@" ;;
  enter)   [ $# -le 1 ] || die "用法: phprt buildroot enter [路径]"; cmd_enter "$@" ;;
  unmount) [ $# -le 1 ] || die "用法: phprt buildroot unmount [路径]"; cmd_unmount "$@" ;;
  remove)  cmd_remove "$@" ;;
  status)  [ $# -le 1 ] || die "用法: phprt buildroot status [路径]"; cmd_status "$@" ;;
  use)     [ $# -eq 1 ] || die "用法: phprt buildroot use <路径>"; cmd_use "$@" ;;
  help|-h|--help) usage ;;
  *) usage; die "未知命令: $cmd" ;;
esac
