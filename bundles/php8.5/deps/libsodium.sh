#!/usr/bin/env bash
# =============================================================================
#  libsodium（sodium 扩展的加密库）
#  phprt: needs=
#  安装到: deps/libsodium-<版本>/
#  可单独执行: lib/modules/deps/libsodium.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of libsodium)"
SIG="libsodium=${LIBSODIUM_VERSION:-};rpath=1"

if [ "$FORCE" != 1 ] && stamp_ok libsodium "$SIG" && [ -f "$D/lib/libsodium.so" ]; then
  ok "libsodium 已安装，跳过"; exit 0
fi

ensure_clean_target libsodium "$D"

TAR="$(basename "$LIBSODIUM_URL")"
fetch "$LIBSODIUM_URL" "$DOWNLOADS/$TAR" "${LIBSODIUM_SHA256:-}"

SRC="$SRCDIR/libsodium-$LIBSODIUM_VERSION"
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
./configure --prefix="$D" --disable-static > "$LOGS/libsodium-configure.log" 2>&1 \
  || { tail -20 "$LOGS/libsodium-configure.log"; die "libsodium configure 失败"; }
make -j"$JOBS" > "$LOGS/libsodium-make.log" 2>&1 \
  || { tail -20 "$LOGS/libsodium-make.log"; die "libsodium make 失败"; }
make install >> "$LOGS/libsodium-make.log" 2>&1

stamp_write libsodium "$SIG"
ok "libsodium $LIBSODIUM_VERSION -> $D"
