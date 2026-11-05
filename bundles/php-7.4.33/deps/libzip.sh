#!/usr/bin/env bash
# =============================================================================
#  libzip（PHP zip 扩展；Composer 解包用）
#  安装到: deps/libzip-<版本>/
#  可单独执行: lib/modules/deps/libzip.sh
#  phprt: needs=openssl
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds cmake make tar

D="$(dep_dir_of libzip)"
SIG="libzip=$LIBZIP_VERSION;zlib=${ENABLE_ZLIB:-0}:${ZLIB_VERSION:-none};openssl=${OPENSSL_VERSION:-none}"

if [ "$FORCE" != 1 ] && stamp_ok libzip "$SIG" && [ -f "$D/lib/libzip.so" ]; then
  ok "libzip 已安装，跳过"; exit 0
fi

ensure_clean_target libzip "$D"

TAR="$(basename "$LIBZIP_URL")"
fetch "$LIBZIP_URL" "$DOWNLOADS/$TAR" "${LIBZIP_SHA256:-}"

SRC="$SRCDIR/libzip-$LIBZIP_VERSION"
BDIR="$SRC/build-local"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  rm -rf "$BDIR"
else
  rm -rf "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

ZLIB_D="$(dep_dir_of zlib)"
OPENSSL_D="$(dep_dir_of openssl)"
ZLIB_FLAG=(-DENABLE_ZLIB=OFF)
PREFIX_PATH="$OPENSSL_D;$D"
[ "${ENABLE_ZLIB:-0}" = 1 ] && { ZLIB_FLAG=(-DENABLE_ZLIB=ON); PREFIX_PATH="$(dep_dir_of zlib);$OPENSSL_D;$D"; }

log "cmake 配置（openssl 提供 zip 加密；关闭 bzip2/lzma/zstd/tools）"
cmake -S "$SRC" -B "$BDIR" \
  -DCMAKE_INSTALL_PREFIX="$D" \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_LIBDIR=lib \
  -DCMAKE_INSTALL_RPATH="$D/lib;$OPENSSL_D/lib;$ZLIB_D/lib" \
  -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
  -DBUILD_SHARED_LIBS=ON \
  "${ZLIB_FLAG[@]}" \
  -DENABLE_BZIP2=OFF -DENABLE_LZMA=OFF -DENABLE_ZSTD=OFF -DENABLE_OPENSSL=ON \
  -DBUILD_TOOLS=OFF -DBUILD_REGRESS=OFF -DBUILD_EXAMPLES=OFF -DBUILD_DOC=OFF \
  -DCMAKE_PREFIX_PATH="$PREFIX_PATH" \
  > "$LOGS/libzip-configure.log" 2>&1 \
  || { tail -30 "$LOGS/libzip-configure.log"; die "libzip cmake 配置失败"; }

log "编译 + 安装"
cmake --build "$BDIR" -j "$JOBS" > "$LOGS/libzip-make.log" 2>&1 \
  || { tail -30 "$LOGS/libzip-make.log"; die "libzip 编译失败"; }
cmake --install "$BDIR" >> "$LOGS/libzip-make.log" 2>&1

stamp_write libzip "$SIG"
ok "libzip $LIBZIP_VERSION -> $D"
