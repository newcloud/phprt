#!/usr/bin/env bash
# buildroot.sh — chroot 构建环境（由 `phprt buildroot` 转发）
#
# 用法：
#   phprt buildroot create [名字|路径] [releasever]   创建/续装 rootfs
#   phprt buildroot enter  [名字|路径] [--root]       进入（exit 自动卸载）
#   phprt buildroot exec   [名字|路径] -- <命令...>   挂载后执行命令，结束自动卸载
#   phprt buildroot unmount [名字|路径]               只卸载挂载
#   phprt buildroot remove [名字|路径] [--dry-run] [--force]  安全删除
#   phprt buildroot status [名字|路径]                查看状态
#   phprt buildroot list                              列出项目的 rootfs
#   phprt buildroot use <名字|路径>                   设为默认
#   phprt buildroot env [chroot|host]                 查看/设置构建环境
#
# 路径规则：
#   - 简单名字（不含 /）：<项目>/.phprt/buildroots/<名字>
#   - 含 / 或以 . 开头：按当前目录解析，允许放到项目外
#   - 省略参数：用 .phprt/buildroots/.config 里的 ROOT_DEFAULT，
#     第一次使用时是 <项目>/.phprt/buildroots/default
#
# 绑定：
#   - 默认绑定项目根和 phprt 仓库，保持绝对路径不变
#   - 自动构建通过 PHPRT_CHROOT_BINDS（冒号分隔）追加 BUILD_ROOT 等目录
#   - rootfs 在项目内时，chroot 里会再看到一次；构建只用精确路径，不做递归扫描
#
# 环境变量：
#   PHPRT_CHROOT_PASS    sudo 密码（默认交互输入）
#   PHPRT_CHROOT_ROOT    临时覆盖默认 rootfs
#   PHPRT_CHROOT_BINDS   额外绑定的目录（冒号分隔）
#   PHPRT_DNF_CACHE      dnf 下载缓存（默认 /var/cache/dnf-phprt）
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MARKER=".phprt-buildroot"

# 项目根：优先 PHPRT_PROJECT；否则向上找 .phprt/current、buildroots 或 php-*
find_project() {
  local d="$PWD"
  while :; do
    if [ -L "$d/.phprt/current" ] || [ -d "$d/.phprt/buildroots" ] ||
       [ -f "$d/.phprt/project.conf" ] ||
       { [ -d "$d/.phprt" ] && compgen -G "$d/.phprt/php-*" >/dev/null; }; then
      printf '%s\n' "$d"
      return 0
    fi
    [ "$d" = "/" ] && break
    d="$(dirname "$d")"
  done
  printf '%s\n' "$PWD"
}

PROJECT_ROOT="${PHPRT_PROJECT:-}"
if [ -z "$PROJECT_ROOT" ]; then
  if [ -n "${PHPRT_HOME:-}" ]; then
    PROJECT_ROOT="$(dirname "$PHPRT_HOME")"
  else
    PROJECT_ROOT="$(find_project)"
  fi
fi
ROOTS_DIR="$PROJECT_ROOT/.phprt/buildroots"
CONF="$ROOTS_DIR/.config"
LOCK="$ROOTS_DIR/.lock"
HOME_BIND="$ROOTS_DIR/.home"

SUDO_PASS="${PHPRT_CHROOT_PASS:-}"
DNF_CACHE="${PHPRT_DNF_CACHE:-/var/cache/dnf-phprt}"
RELVER_DEFAULT="$(. /etc/os-release 2>/dev/null && printf '%s' "${VERSION_ID:-}")"

ROOT_DEFAULT=""
BUILD_ENV="host"
BINDS=()
MOUNTS=()
CLEANUP_ROOT=""
CHROOT_PID=""

log() { printf '[buildroot] %s\n' "$*"; }
warn() { printf '  ! %s\n' "$*" >&2; }
die() { printf '错误: %s\n' "$*" >&2; exit 1; }

# 递归终止 chroot 里的进程树；只杀 PID 可能留下 make/gcc 子进程，
# 挂载就卸不掉。
kill_tree() {
  local pid="$1" c
  if [ -r "/proc/$pid/task/$pid/children" ]; then
    for c in $(<"/proc/$pid/task/$pid/children"); do
      kill_tree "$c"
    done
  fi
  kill -TERM "$pid" 2>/dev/null || true
}

on_signal() {
  local code="$1"
  trap - INT TERM HUP EXIT
  [ -n "${CHROOT_PID:-}" ] && kill_tree "$CHROOT_PID"
  [ -n "${CHROOT_PID:-}" ] && wait "$CHROOT_PID" 2>/dev/null || true
  CHROOT_PID=""
  cleanup_mounts || true
  exit "$code"
}

usage() {
  awk 'NR >= 4 {
    if ($0 ~ /^set -euo pipefail/) exit
    sub(/^# ?/, "")
    print
  }' "${BASH_SOURCE[0]}"
}

run_root() {
  if [ "$(id -u)" = 0 ]; then
    "$@"
  elif [ -n "$SUDO_PASS" ]; then
    # sudo_auth 已经缓存过票据时用普通 sudo，保留调用者的 stdin
    # （enter 的交互 shell 需要）；票据过期才退回 -S。
    if sudo -n true 2>/dev/null; then
      sudo "$@"
    else
      printf '%s\n' "$SUDO_PASS" | sudo -S -p '' "$@"
    fi
  else
    sudo "$@"
  fi
}

sudo_auth() {
  [ "$(id -u)" = 0 ] && return 0
  if [ -n "$SUDO_PASS" ]; then
    printf '%s\n' "$SUDO_PASS" | sudo -S -p '' -v >/dev/null 2>&1 ||
      die "sudo 密码错误"
  else
    sudo -v
  fi
}

load_conf() {
  ROOT_DEFAULT=""
  BUILD_ENV="host"
  if [ -f "$CONF" ]; then
    # shellcheck disable=SC1090
    . "$CONF"
  fi
  BUILD_ENV="${BUILD_ENV:-host}"
}

write_conf() {  # [root] [env]
  local root="${1:-${ROOT_DEFAULT:-$ROOTS_DIR/default}}"
  local env="${2:-${BUILD_ENV:-host}}"
  mkdir -p "$ROOTS_DIR"
  printf 'ROOT_DEFAULT=%q\nBUILD_ENV=%q\n' "$root" "$env" > "$CONF"
  ROOT_DEFAULT="$root"
  BUILD_ENV="$env"
}

resolve_path() {
  local arg="${1:-}"
  if [ -z "$arg" ]; then
    if [ -n "${PHPRT_CHROOT_ROOT:-}" ]; then
      realpath -m -- "$PHPRT_CHROOT_ROOT"
      return
    fi
    load_conf
    if [ -n "${ROOT_DEFAULT:-}" ]; then
      printf '%s\n' "$ROOT_DEFAULT"
    else
      printf '%s/default\n' "$ROOTS_DIR"
    fi
    return
  fi
  case "$arg" in
    */*|.*) realpath -m -- "$arg" ;;
    *)      realpath -m -- "$ROOTS_DIR/$arg" ;;
  esac
}

looks_rootfs() {
  [ -f "$1/etc/os-release" ] && [ -x "$1/usr/bin/bash" ]
}

is_created() { [ -f "$1/$MARKER" ]; }

take_lock() {
  mkdir -p "$ROOTS_DIR"
  command -v flock >/dev/null 2>&1 || return 0
  exec 9>"$LOCK"
  flock -n 9 || die "另一个 buildroot 操作正在进行（锁: $LOCK）"
}

# 收集默认和额外 bind 目录；嵌套路径只保留外层，避免重复挂载
collect_binds() {
  local -a cand=("$PROJECT_ROOT" "$REPO_ROOT")
  local p q
  if [ -n "${PHPRT_CHROOT_BINDS:-}" ]; then
    local -a extra=()
    IFS=: read -r -a extra <<< "$PHPRT_CHROOT_BINDS"
    cand+=("${extra[@]}")
  fi
  for p in "$@"; do
    [ -n "$p" ] && cand+=("$p")
  done

  BINDS=()
  for p in "${cand[@]}"; do
    [ -n "$p" ] || continue
    p="$(realpath -m -- "$p" 2>/dev/null || true)"
    [ -n "$p" ] || continue
    [ "$p" != "/" ] || die "绑定目录不能是 /"
    [ -d "$p" ] || die "绑定目录不存在: $p"
    local dup=0 q
    for q in "${BINDS[@]}"; do
      [ "$p" = "$q" ] && { dup=1; break; }
    done
    [ "$dup" = 1 ] || BINDS+=("$p")
  done

  # 去掉被另一个 bind 覆盖的路径
  local -a kept=()
  local skip
  for p in "${BINDS[@]}"; do
    skip=0
    for q in "${BINDS[@]}"; do
      [ "$p" = "$q" ] && continue
      case "$p/" in "$q"/*) skip=1; break ;; esac
    done
    [ "$skip" = 1 ] || kept+=("$p")
  done
  BINDS=("${kept[@]}")
}

mount_count() {
  local root="$1"
  if command -v findmnt >/dev/null 2>&1; then
    findmnt -rn -o TARGET 2>/dev/null |
      awk -v r="$root" '$0 == r || index($0, r "/") == 1' | wc -l
  else
    mount | grep -c -- "$root" 2>/dev/null || true
  fi
}

show_mounts() {
  local root="$1"
  if command -v findmnt >/dev/null 2>&1; then
    findmnt -rn -o TARGET,SOURCE 2>/dev/null |
      awk -v r="$root" '$1 == r || index($1, r "/") == 1 { printf "    %s <- %s\n", $1, $2 }'
  else
    mount | grep -- "$root" || true
  fi
}

chroot_procs() {
  local real="$1"
  run_root sh -c '
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

track_mount() { MOUNTS+=("$1"); }

# 正常 root 用 mknod；用户命名空间/容器里没 CAP_MKNOD 时退回 bind 宿主设备节点。
make_dev_node() {
  local root="$1" name="$2" type="$3" major="$4" minor="$5"
  if run_root mknod -m 666 "$root/dev/$name" "$type" "$major" "$minor" 2>/dev/null; then
    return 0
  fi
  if [ -e "/dev/$name" ]; then
    run_root touch "$root/dev/$name"
    run_root mount --bind "/dev/$name" "$root/dev/$name"
    track_mount "$root/dev/$name"
    warn "mknod 失败，已 bind 宿主 /dev/$name"
  else
    warn "无法创建 /dev/$name"
  fi
}

# 按深度倒序卸载 root 下所有挂载；用于 unmount 和异常兜底
unmount_under() {
  local root="$1" t
  local -a targets=()
  if command -v findmnt >/dev/null 2>&1; then
    while IFS= read -r t; do
      [ -n "$t" ] && targets+=("$t")
    done < <(findmnt -rn -o TARGET 2>/dev/null |
      awk -v r="$root" '$0 == r || index($0, r "/") == 1' |
      awk '{ print length($0) "\t" $0 }' | sort -rn | cut -f2-)
  fi
  for t in "${targets[@]}"; do
    [ "$t" = "$root" ] && continue
    run_root umount -R "$t" 2>/dev/null || run_root umount -l "$t" 2>/dev/null || true
  done
}

mount_all() {
  local root="$1"
  local uid gid
  uid="$(id -u)"
  gid="$(id -g)"
  MOUNTS=()
  CLEANUP_ROOT="$root"

  run_root mkdir -p "$root/proc" "$root/sys" "$root/dev" "$root/dev/pts" "$root/dev/shm"
  run_root mount -t proc proc "$root/proc"; track_mount "$root/proc"
  if run_root mount -t sysfs sys "$root/sys" 2>/dev/null; then
    track_mount "$root/sys"
  else
    warn "sysfs 挂载失败，继续（构建通常不需要 /sys）"
  fi

  run_root mount -t tmpfs -o mode=755,size=16m tmpfs "$root/dev"; track_mount "$root/dev"
  run_root mkdir -p "$root/dev/pts" "$root/dev/shm"
  run_root mount -t devpts -o newinstance,ptmxmode=0666,mode=0620 devpts "$root/dev/pts"
  track_mount "$root/dev/pts"
  run_root ln -sfn pts/ptmx "$root/dev/ptmx"
  run_root mount -t tmpfs -o mode=1777,size=256m tmpfs "$root/dev/shm"
  track_mount "$root/dev/shm"
  make_dev_node "$root" null    c 1 3
  make_dev_node "$root" zero    c 1 5
  make_dev_node "$root" full    c 1 7
  make_dev_node "$root" random  c 1 8
  make_dev_node "$root" urandom c 1 9
  make_dev_node "$root" tty     c 5 0
  run_root ln -sfn /proc/self/fd   "$root/dev/fd"
  run_root ln -sfn /proc/self/fd/0 "$root/dev/stdin"
  run_root ln -sfn /proc/self/fd/1 "$root/dev/stdout"
  run_root ln -sfn /proc/self/fd/2 "$root/dev/stderr"

  local p
  for p in "${BINDS[@]}"; do
    run_root mkdir -p "$root$p"
    run_root mount --bind "$p" "$root$p"
    track_mount "$root$p"
  done

  # rootfs 在项目内时，chroot 里会通过项目 bind 再看到它一次。构建只使用
  # 精确路径，不做递归扫描；不在项目 bind 上再叠 tmpfs，避免把宿主
  # .phprt/buildroots 也盖住。

  # 构建用户的 HOME，单独挂到 /home/phprt
  mkdir -p "$HOME_BIND"
  chmod 700 "$HOME_BIND" 2>/dev/null || true
  run_root mkdir -p "$root/home/phprt"
  run_root mount --bind "$HOME_BIND" "$root/home/phprt"
  track_mount "$root/home/phprt"

  ROOT_UID="$uid"
  ROOT_GID="$gid"
}

cleanup_mounts() {
  [ -n "${CLEANUP_ROOT:-}" ] || return 0
  local root="$CLEANUP_ROOT" i
  for ((i = ${#MOUNTS[@]} - 1; i >= 0; i--)); do
    run_root umount -R "${MOUNTS[i]}" 2>/dev/null ||
      run_root umount -l "${MOUNTS[i]}" 2>/dev/null || true
  done
  MOUNTS=()
  # 兜底：异常退出时按 findmnt 再清一遍，处理没进 MOUNTS 的残留
  unmount_under "$root" || true
  CLEANUP_ROOT=""
}

run_chroot() {  # $1=root $2=as_root(0/1) $3=cwd；其余为命令
  local root="$1" as_root="$2" cwd="$3"
  shift 3
  local uid gid user
  uid="$(id -u)"; gid="$(id -g)"; user="$(id -un 2>/dev/null || printf '%s' "$uid")"

  local -a envs=(
    HOME=/home/phprt
    USER="$user"
    LOGNAME="$user"
    PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
    TMPDIR=/tmp
  )
  local v
  for v in PHPRT_REPO PHPRT_RUNNER PHPRT_PROJECT PHPRT_HOME PHPRT_WORK \
           PHPRT_SEED PHPRT_DOWNLOADS PHPRT_PHP_VERSION PHPRT_EXT_MODES \
           PHPRT_FORCE_PHP PHPRT_CLEAN_SRC PHPRT_LINK_MODE PHPRT_LTO \
           BUILD_ROOT BUNDLE TREE PREFIX DEPS SELECTED_EXTS DOWNLOADS VENDOR \
           JOBS OFFLINE PHPRT_PROXY https_proxy HTTPS_PROXY \
           http_proxy HTTP_PROXY NO_PROXY no_proxy GITHUB_TOKEN; do
    [ -n "${!v:-}" ] && envs+=("$v=${!v}")
  done

  local -a userarg=()
  if [ "$as_root" != 1 ]; then
    userarg=(--userspec="$uid:$gid")
  fi

  run_root chroot "${userarg[@]}" "$root" /usr/bin/env -i "${envs[@]}" \
    /bin/bash -c 'cd "$1" || exit 1; shift; exec "$@"' bash "$cwd" "$@"
}

# ---------- create ----------
check_create_target() {
  local root="$1"
  [ "$root" != "/" ] || die "拒绝使用 / 作为 rootfs"
  [ "$root" != "$HOME" ] || die "拒绝使用 \$HOME 作为 rootfs"
  case "$root" in
    "$REPO_ROOT/.phprt/buildroots"/*) ;;
    "$REPO_ROOT"|"$REPO_ROOT"/*)
      die "拒绝写入 phprt 仓库目录: $root" ;;
  esac
}

cmd_create() {
  local root relver
  root="$(resolve_path "${1:-}")"
  relver="${2:-$RELVER_DEFAULT}"
  collect_binds
  check_create_target "$root"

  if [ -e "$root" ] && [ ! -d "$root" ]; then
    die "已存在同名文件，不能作为 rootfs: $root"
  fi
  if [ -d "$root" ] && [ -n "$(ls -A "$root" 2>/dev/null)" ] && ! looks_rootfs "$root"; then
    die "目录已存在且不像 rootfs，拒绝写入: $root"
  fi

  take_lock
  sudo_auth
  [ "$(mount_count "$root")" -eq 0 ] ||
    die "rootfs 内仍有挂载，先跑：phprt buildroot unmount $root"

  run_root mkdir -p -- "$root"
  log "dnf --installroot=$root（releasever=${relver:-跟随系统}）"
  log "  dnf 下载缓存: $DNF_CACHE"
  run_root mkdir -p -- "$DNF_CACHE"
  local -a relargs=()
  [ -n "$relver" ] && relargs=(--releasever="$relver")
  run_root dnf -y \
    --installroot="$root" --use-host-config "${relargs[@]}" \
    --setopt=install_weak_deps=False \
    --setopt=keepcache=True \
    --setopt=cachedir="$DNF_CACHE" \
    --setopt=max_parallel_downloads=10 \
    install gcc gcc-c++ make cmake patchelf \
      pkgconf-pkg-config perl m4 bison re2c \
      tar xz bzip2 unzip curl binutils diffutils gawk \
      openssl util-linux ca-certificates \
      >/dev/null 9>&-
  log "依赖安装完成（缓存: $(run_root du -sh -- "$DNF_CACHE" 2>/dev/null | cut -f1)）"

  run_root rm -f "$root/etc/resolv.conf"
  run_root cp -L /etc/resolv.conf "$root/etc/resolv.conf"

  # 让 enter / build 里的 id、whoami 能认出宿主用户（uid 仍由 --userspec 决定）
  if [ "$(id -u)" != 0 ]; then
    run_root sh -c '
      u="$1" g="$2" un="$3" gn="$4" root="$5"
      mkdir -p "$root/home/phprt"
      grep -q "^$un:" "$root/etc/passwd" 2>/dev/null ||
        printf "%s:x:%s:%s::/home/phprt:/bin/bash\n" "$un" "$u" "$g" >> "$root/etc/passwd"
      grep -q "^$gn:" "$root/etc/group" 2>/dev/null ||
        printf "%s:x:%s:\n" "$gn" "$g" >> "$root/etc/group"
    ' sh "$(id -u)" "$(id -g)" "$(id -un)" "$(id -gn)" "$root"
  fi

  run_root sh -c 'printf "created_at=%s\n" "$1" > "$2"' sh \
    "$(date -Is)" "$root/$MARKER"

  load_conf
  write_conf "$root" "${BUILD_ENV:-host}"
  log "完成：$root（$(run_root du -sh -- "$root" 2>/dev/null | cut -f1)）"
  log "下一步：phprt buildroot enter $root"
}

# ---------- enter / exec ----------
cmd_enter() {
  local root_arg="" as_root=0 a
  for a in "$@"; do
    case "$a" in
      --root) as_root=1 ;;
      -*) die "未知参数: $a" ;;
      *) [ -z "$root_arg" ] || die "多余参数: $a"; root_arg="$a" ;;
    esac
  done
  local root; root="$(resolve_path "$root_arg")"
  collect_binds
  [ -d "$root" ] || die "rootfs 不存在: $root（先跑：phprt buildroot create）"
  looks_rootfs "$root" || die "目录不像 rootfs: $root"
  [ "${PHPRT_CHROOT_AS_ROOT:-0}" = 1 ] && as_root=1

  take_lock
  load_conf
  write_conf "$root" "${BUILD_ENV:-host}"
  sudo_auth
  [ "$(mount_count "$root")" -eq 0 ] ||
    die "rootfs 内已有挂载（上次未正常退出？），先跑：phprt buildroot unmount $root"

  trap cleanup_mounts EXIT
  mount_all "$root"
  log "进入 $root（exit 退出，自动卸载）"
  local rc=0
  run_chroot "$root" "$as_root" "$PROJECT_ROOT" /bin/bash --login 9>&- || rc=$?
  cleanup_mounts
  local n; n="$(mount_count "$root")"
  if [ "$n" -eq 0 ]; then
    log "已退出，挂载已全部卸载"
  else
    log "警告: 仍有 $n 个挂载残留，执行：phprt buildroot unmount $root"
    show_mounts "$root" >&2
  fi
  return "$rc"
}

cmd_exec() {
  local root_arg="" as_root=0 a
  local -a cmds=() extra=()
  while [ $# -gt 0 ]; do
    case "$1" in
      --root) as_root=1; shift ;;
      --bind) [ $# -ge 2 ] || die "--bind 需要参数"; extra+=("$2"); shift 2 ;;
      --)
        shift
        cmds=("$@")
        break
        ;;
      -*) die "未知参数: $1" ;;
      *)
        if [ -z "$root_arg" ]; then root_arg="$1"; else die "命令请放在 -- 之后"; fi
        shift
        ;;
    esac
  done
  [ ${#cmds[@]} -gt 0 ] || die "用法: phprt buildroot exec [名字|路径] -- <命令...>"

  local root; root="$(resolve_path "$root_arg")"
  collect_binds "${extra[@]}"
  [ -d "$root" ] || die "rootfs 不存在: $root（先跑：phprt buildroot create）"
  looks_rootfs "$root" || die "目录不像 rootfs: $root"
  [ "${PHPRT_CHROOT_AS_ROOT:-0}" = 1 ] && as_root=1

  take_lock
  sudo_auth
  [ "$(mount_count "$root")" -eq 0 ] ||
    die "rootfs 内已有挂载，先跑：phprt buildroot unmount $root"

  trap cleanup_mounts EXIT
  mount_all "$root"
  local rc=0 cwd="$PWD" p inside=0
  case "$cwd/" in
    "$root"/*) cwd="$PROJECT_ROOT" ;;
    *)
      for p in "${BINDS[@]}"; do
        case "$cwd/" in "$p"/*) inside=1; break ;; esac
      done
      [ "$inside" = 1 ] || cwd="$PROJECT_ROOT"
      ;;
  esac
  if [ "${PHPRT_CHROOT_DETACH:-0}" = 1 ]; then
    run_chroot "$root" "$as_root" "$cwd" "${cmds[@]}" 9>&- &
    CHROOT_PID=$!
    wait "$CHROOT_PID" || rc=$?
    CHROOT_PID=""
  else
    run_chroot "$root" "$as_root" "$cwd" "${cmds[@]}" 9>&- || rc=$?
  fi
  cleanup_mounts
  exit "$rc"
}

# ---------- unmount ----------
cmd_unmount() {
  local root; root="$(resolve_path "${1:-}")"
  [ -d "$root" ] || die "路径不存在: $root"
  sudo_auth
  log "卸载前挂载数: $(mount_count "$root")"
  unmount_under "$root"
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
check_remove_target() {
  local root="$1"
  [ ! -L "$root" ] || die "拒绝删除：$root 是符号链接"
  [ "$root" != "/" ] || die "拒绝删除 /"
  [ "$root" != "$HOME" ] || die "拒绝删除 \$HOME"
  [ -d "$root" ] || return 0
  is_created "$root" || die "拒绝删除：$root 不是 phprt buildroot create 创建的（缺少 $MARKER）"
  looks_rootfs "$root" || die "拒绝删除：$root 不像 rootfs"
}

cmd_remove() {
  local path="" dry=0 force=0 a
  for a in "$@"; do
    case "$a" in
      --dry-run) dry=1 ;;
      --force) force=1 ;;
      -*) die "未知参数: $a" ;;
      *) [ -z "$path" ] || die "多余参数: $a"; path="$a" ;;
    esac
  done
  local root; root="$(resolve_path "$path")"
  check_remove_target "$root"

  if [ ! -d "$root" ]; then
    log "不存在，无需删除: $root"
    load_conf
    if [ "${ROOT_DEFAULT:-}" = "$root" ]; then
      write_conf "$ROOTS_DIR/default" "${BUILD_ENV:-host}"
    fi
    return 0
  fi

  sudo_auth
  local procs; procs="$(chroot_procs "$root" | tr '\n' ' ' | sed 's/ *$//')"
  if [ "$dry" -eq 1 ]; then
    echo "== dry-run：不做任何修改 =="
    echo "  rootfs    : $root"
    echo "  大小      : $(run_root du -sh -- "$root" 2>/dev/null | cut -f1)"
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
  unmount_under "$root"
  local n; n="$(mount_count "$root")"
  if [ "$n" -ne 0 ]; then
    show_mounts "$root" >&2
    die "仍有 $n 个挂载残留，未做任何删除"
  fi
  log "挂载已清零，删除 $root"
  run_root rm -rf --one-file-system -- "$root"
  log "已删除: $root"

  load_conf
  if [ "${ROOT_DEFAULT:-}" = "$root" ]; then
    write_conf "$ROOTS_DIR/default" "${BUILD_ENV:-host}"
    log "已重置默认路径: $ROOTS_DIR/default"
  fi
}

# ---------- status / list / use / env ----------
cmd_status() {
  local root; root="$(resolve_path "${1:-}")"
  load_conf
  local cur="${ROOT_DEFAULT:-}"
  echo "rootfs    : $root"
  if [ -n "$cur" ] && [ "$root" = "$cur" ]; then
    echo "默认      : 是"
  else
    echo "默认      : 否${cur:+（当前默认 $cur）}"
  fi
  echo "构建环境  : ${BUILD_ENV:-host}"
  if [ ! -d "$root" ]; then echo "状态      : 不存在"; return 0; fi
  if looks_rootfs "$root"; then
    if is_created "$root"; then echo "状态      : 可用"; else echo "状态      : 可用（不是 create 创建的）"; fi
  else
    echo "状态      : 目录存在但不像 rootfs"
    return 0
  fi
  sudo_auth
  echo "大小      : $(run_root du -sh -- "$root" 2>/dev/null | cut -f1)"
  echo "包数      : $(run_root rpm --root "$root" -qa 2>/dev/null | wc -l)"
  echo "挂载      : $(mount_count "$root") 个"
  show_mounts "$root"
  local p
  p="$(chroot_procs "$root" | tr '\n' ' ' | sed 's/ *$//')"
  echo "使用进程  : ${p:-无}"
}

cmd_list() {
  load_conf
  [ -d "$ROOTS_DIR" ] || { echo "（还没有 buildroots）"; return 0; }
  local d n
  for d in "$ROOTS_DIR"/*; do
    [ -d "$d" ] || continue
    is_created "$d" || continue
    n="$(basename "$d")"
    if [ "${ROOT_DEFAULT:-}" = "$d" ]; then
      printf '* %s\n' "$n"
    else
      printf '  %s\n' "$n"
    fi
  done
}

cmd_use() {
  local root; root="$(resolve_path "${1:-}")"
  [ -d "$root" ] || die "路径不存在: $root"
  looks_rootfs "$root" || die "目录不像 rootfs: $root"
  load_conf
  write_conf "$root" "${BUILD_ENV:-host}"
  log "默认 rootfs: $root"
}

cmd_env() {
  load_conf
  case "${1:-}" in
    "")
      printf '%s\n' "${BUILD_ENV:-host}"
      ;;
    host|chroot)
      if [ "$1" = chroot ]; then
        local root; root="$(resolve_path "")"
        [ -d "$root" ] && looks_rootfs "$root" ||
          die "没有可用的 rootfs，先跑：phprt buildroot create"
        write_conf "${ROOT_DEFAULT:-$(resolve_path "")}" chroot
      elif [ -f "$CONF" ]; then
        # 已经是 host 时不创建空的 buildroots，只有切回时才落盘
        write_conf "${ROOT_DEFAULT:-$(resolve_path "")}" host
      fi
      log "构建环境: $1"
      ;;
    *) die "用法: phprt buildroot env [chroot|host]" ;;
  esac
}

# ---------- dispatch ----------
trap 'on_signal 130' INT
trap 'on_signal 143' TERM
trap 'on_signal 129' HUP
trap cleanup_mounts EXIT

cmd="${1:-help}"
[ $# -gt 0 ] && shift
case "$cmd" in
  create)  [ $# -le 2 ] || die "用法: phprt buildroot create [名字|路径] [releasever]"; cmd_create "$@" ;;
  enter)   cmd_enter "$@" ;;
  exec)    cmd_exec "$@" ;;
  unmount) [ $# -le 1 ] || die "用法: phprt buildroot unmount [名字|路径]"; cmd_unmount "$@" ;;
  remove)  cmd_remove "$@" ;;
  status)  [ $# -le 1 ] || die "用法: phprt buildroot status [名字|路径]"; cmd_status "$@" ;;
  list)    [ $# -eq 0 ] || die "用法: phprt buildroot list"; cmd_list ;;
  use)     [ $# -eq 1 ] || die "用法: phprt buildroot use <名字|路径>"; cmd_use "$@" ;;
  env)     [ $# -le 1 ] || die "用法: phprt buildroot env [chroot|host]"; cmd_env "$@" ;;
  help|-h|--help) usage ;;
  *) usage; die "未知命令: $cmd" ;;
esac
