#!/usr/bin/env bash
# setup-build-chroot.sh — 用 dnf --installroot 创建能编译 phprt 的 chroot 环境
#
# 原理：dnf --installroot 把系统装到 <仓库>/buildroot/，再用 chroot + 绑定挂载
#       获得隔离的编译环境；项目父目录绑定进 chroot 后路径不变，phprt/blog 直接可用。
#
# 用法：
#   ./scripts/setup-build-chroot.sh [chroot路径] [releasever]
#     chroot路径  默认 <仓库>/buildroot
#     releasever  默认与当前系统 VERSION_ID 一致（如需其它版本再指定）
#
# 生成的脚本（放在仓库根，与 buildroot/ 同级）：
#   enter-build-chroot.sh     进入环境（退出时自动卸载所有挂载）
#   unmount-build-chroot.sh   卸载全部挂载（proc/sys/dev/pts/项目绑定）
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
CHROOT="${1:-$HOST_PROJECT/buildroot}"
RELVER="${2:-$(. /etc/os-release 2>/dev/null && echo "${VERSION_ID:-}")}"
SUDO_PASS="${PHPRT_CHROOT_PASS:-}"

log() { printf '[chroot] %s\n' "$*"; }

# 0. 验证 sudo（给了密码变量就非交互，否则交互输入）
sudo_auth() {
  if [ -n "$SUDO_PASS" ]; then
    echo "$SUDO_PASS" | sudo -S -v >/dev/null 2>&1 || { echo "sudo 密码错误" >&2; exit 1; }
  else
    sudo -v
  fi
}
sudo_auth

# 1. 安装构建依赖（最小显式列表，不用 @development-tools 组：
#    该组会拖入 mesa/llvm/clang/doxygen/git-doc 等几百 MB 无关包）
#    依据：README「前置依赖」表 + runner 脚本实际调用的命令。
#    git 不装（下载全走 curl）；autoconf/re2c 不装（phprt 自动装到 tools/）。
log "dnf --installroot=$CHROOT 安装构建依赖（releasever 默认跟随当前系统）..."
RELVER_ARGS=()
[ -n "$RELVER" ] && RELVER_ARGS=(--releasever="$RELVER")
sudo dnf -y \
  --installroot="$CHROOT" --use-host-config "${RELVER_ARGS[@]}" \
  --setopt=install_weak_deps=False \
  install gcc gcc-c++ make cmake patchelf \
    pkgconf-pkg-config perl m4 bison re2c \
    tar xz bzip2 unzip curl binutils diffutils gawk openssl util-linux \
    >/dev/null
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
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$HERE/buildroot"
BIND="${PHPRT_CHROOT_BIND:-$(cd "$HERE/.." && pwd)}"
PASS="${PHPRT_CHROOT_PASS:-}"
if [ -n "$PASS" ]; then echo "$PASS" | sudo -S -v; else sudo -v; fi
sudo mount -t proc    proc    "$ROOT/proc"
sudo mount -t sysfs   sys     "$ROOT/sys"
sudo mount --rbind    /dev    "$ROOT/dev"
sudo mount --make-rslave "$ROOT/dev"
sudo mount -t devpts  devpts  "$ROOT/dev/pts"
sudo mkdir -p "$ROOT$BIND"
sudo mount --bind     "$BIND" "$ROOT$BIND"
cleanup() {
  sudo umount -R "$ROOT$BIND" "$ROOT/dev/pts" "$ROOT/dev" \
                 "$ROOT/proc" "$ROOT/sys" 2>/dev/null || true
}
trap cleanup EXIT
echo "== 进入 chroot（exit 退出） =="
sudo chroot "$ROOT" /bin/bash --login
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

left() { mount | grep -c "$ROOT" 2>/dev/null || true; }
echo "卸载前挂载数: $(left)"

# 子挂载在前，逐个卸载；rbind 下整体 -R 可能失败，失败就 lazy 单点卸
for m in "$BIND" dev/pts dev/shm dev proc sys; do
  if mountpoint -q "$ROOT$m" 2>/dev/null; then
    sudo umount -R "$ROOT$m" 2>/dev/null || sudo umount -l "$ROOT$m" 2>/dev/null || true
  fi
done

# 兜底：仍残留则对整个 root lazy 递归卸载
[ "$(left)" -eq 0 ] || sudo umount -R -l "$ROOT" 2>/dev/null || true

echo "卸载后挂载数: $(left)"
if [ "$(left)" -eq 0 ]; then
  echo "OK: chroot 挂载已全部卸载"
else
  echo "警告: 仍有挂载残留:" >&2
  mount | grep "$ROOT" >&2 || true
  exit 1
fi
EOF
chmod +x "$HOST_PROJECT/unmount-build-chroot.sh"

log "完成。进入环境：$HOST_PROJECT/enter-build-chroot.sh"
log "  卸载挂载：$HOST_PROJECT/unmount-build-chroot.sh"
log "  chroot 根目录: $CHROOT"
log "  绑定目录: $BIND_ROOT（chroot 内同路径；可用 PHPRT_CHROOT_BIND 覆盖）"
