#!/usr/bin/env bash
# =============================================================================
#  libffi（PHP FFI 扩展的依赖）
#  安装到: deps/libffi-<版本>/
#  可单独执行: lib/modules/libffi.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of libffi)"
SIG="libffi=$LIBFFI_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok libffi "$SIG" && [ -f "$D/lib/libffi.so" ]; then
  ok "libffi 已安装，跳过"; exit 0
fi

ensure_clean_target libffi "$D"

TAR="$(basename "$LIBFFI_URL")"
fetch "$LIBFFI_URL" "$DOWNLOADS/$TAR" "${LIBFFI_SHA256:-}"

SRC="$SRCDIR/libffi-$LIBFFI_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

cd "$SRC"
log "configure + make -j$JOBS"
./configure --prefix="$D" --disable-static > "$LOGS/libffi-configure.log" 2>&1 \
  || { tail -20 "$LOGS/libffi-configure.log"; die "libffi configure 失败"; }
make -j"$JOBS" > "$LOGS/libffi-make.log" 2>&1 \
  || { tail -20 "$LOGS/libffi-make.log"; die "libffi make 失败"; }
make install >> "$LOGS/libffi-make.log" 2>&1

stamp_write libffi "$SIG"
ok "libffi $LIBFFI_VERSION -> $D"
