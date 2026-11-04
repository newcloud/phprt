#!/usr/bin/env bash
# =============================================================================
#  libpng（gd 的 PNG 支持，依赖 zlib）
#  phprt: needs=zlib
#  安装到: deps/libpng-<版本>/
#  可单独执行: lib/modules/deps/libpng.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of libpng)"
SIG="libpng=$LIBPNG_VERSION;zlib=$ZLIB_VERSION;rpath=1"

if [ "$FORCE" != 1 ] && stamp_ok libpng "$SIG" && [ -f "$D/lib/libpng16.so" ]; then
  ok "libpng 已安装，跳过"; exit 0
fi

ensure_needs libpng
dep_env libpng

ensure_clean_target libpng "$D"

TAR="$(basename "$LIBPNG_URL")"
fetch "$LIBPNG_URL" "$DOWNLOADS/$TAR" "${LIBPNG_SHA256:-}"

SRC="$SRCDIR/libpng-$LIBPNG_VERSION"
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
./configure --prefix="$D" --disable-static > "$LOGS/libpng-configure.log" 2>&1 \
  || { tail -20 "$LOGS/libpng-configure.log"; die "libpng configure 失败"; }
make -j"$JOBS" > "$LOGS/libpng-make.log" 2>&1 \
  || { tail -20 "$LOGS/libpng-make.log"; die "libpng make 失败"; }
make install >> "$LOGS/libpng-make.log" 2>&1

stamp_write libpng "$SIG"
ok "libpng $LIBPNG_VERSION -> $D"
