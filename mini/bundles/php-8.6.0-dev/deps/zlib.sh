#!/usr/bin/env bash
# =============================================================================
#  zlib（PHP 的 zlib/gz 函数、libxml2、libcurl、libzip 都依赖它）
#  安装到: deps/zlib-<版本>/
#  可单独执行: lib/modules/deps/zlib.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of zlib)"
SIG="zlib=$ZLIB_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok zlib "$SIG" && [ -f "$D/lib/libz.so" ]; then
  ok "zlib 已安装，跳过"; exit 0
fi

ensure_clean_target zlib "$D"

TAR="$(basename "$ZLIB_URL")"
fetch "$ZLIB_URL" "$DOWNLOADS/$TAR" "${ZLIB_SHA256:-}"

SRC="$SRCDIR/zlib-$ZLIB_VERSION"
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
./configure --prefix="$D" > "$LOGS/zlib-configure.log" 2>&1 \
  || { tail -20 "$LOGS/zlib-configure.log"; die "zlib configure 失败"; }
make -j"$JOBS" > "$LOGS/zlib-make.log" 2>&1 \
  || { tail -20 "$LOGS/zlib-make.log"; die "zlib make 失败"; }
make install >> "$LOGS/zlib-make.log" 2>&1

stamp_write zlib "$SIG"
ok "zlib $ZLIB_VERSION -> $D"
