#!/usr/bin/env bash
# =============================================================================
#  libxml2（PHP dom / xml / simplexml / soap）
#  安装到: deps/libxml2-<版本>/
#  可单独执行: lib/modules/deps/libxml2.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of libxml2)"
SIG="libxml2=$LIBXML2_VERSION;zlib=${ENABLE_ZLIB:-0}"

if [ "$FORCE" != 1 ] && stamp_ok libxml2 "$SIG" && [ -f "$D/lib/libxml2.so" ]; then
  ok "libxml2 已安装，跳过"; exit 0
fi

ensure_clean_target libxml2 "$D"

TAR="$(basename "$LIBXML2_URL")"
fetch "$LIBXML2_URL" "$DOWNLOADS/$TAR" "${LIBXML2_SHA256:-}"

SRC="$SRCDIR/libxml2-$LIBXML2_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

ZLIB_ARG=(--without-zlib)
[ "${ENABLE_ZLIB:-0}" = 1 ] && ZLIB_ARG=(--with-zlib="$(dep_dir_of zlib)")

cd "$SRC"
export PKG_CONFIG_PATH="$(dep_pc_path zlib)${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
log "configure + make -j$JOBS"
./configure --prefix="$D" "${ZLIB_ARG[@]}" \
    --without-lzma --without-python --without-http --without-ftp \
    > "$LOGS/libxml2-configure.log" 2>&1 \
  || { tail -30 "$LOGS/libxml2-configure.log"; die "libxml2 configure 失败"; }
make -j"$JOBS" > "$LOGS/libxml2-make.log" 2>&1 \
  || { tail -20 "$LOGS/libxml2-make.log"; die "libxml2 make 失败"; }
make install >> "$LOGS/libxml2-make.log" 2>&1

stamp_write libxml2 "$SIG"
ok "libxml2 $LIBXML2_VERSION -> $D"
