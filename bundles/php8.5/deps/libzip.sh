#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=libzip version=LIBZIP_VERSION needs=zlib optional=openssl tools= wheel=auto upstream=github:nih-at/libzip
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# zlib 总是需要；openssl 只在开关打开时需要（提供 zip 加密）；
# bzip2 只在 zip 的 bzip2 子项打开时需要。
needs() {
  printf '%s\n' zlib
  [ "${ENABLE_OPENSSL:-0}" = 1 ] && printf '%s\n' openssl
  [ "${ENABLE_LIBZIP_BZIP2:-0}" = 1 ] && printf '%s\n' bzip2
  return 0
}

build() {
  need_cmds cmake make tar

  local D SIG TAR SRC BDIR ZLIB_D OPENSSL_D BZIP2_D
  D="$(dep_dir_of libzip)"
  SIG="libzip=$LIBZIP_VERSION;zlib=1:${ZLIB_VERSION:-none};openssl=${OPENSSL_VERSION:-none};bzip2=${ENABLE_LIBZIP_BZIP2:-0}:${BZIP2_VERSION:-none}"

  if [ "$FORCE" != 1 ] && stamp_ok libzip "$SIG" && [ -f "$D/lib/libzip.so" ]; then
    ok "libzip 已安装，跳过"; return 0
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
  BZIP2_D=""
  [ "${ENABLE_LIBZIP_BZIP2:-0}" = 1 ] && BZIP2_D="$(dep_dir_of bzip2)"

  log "cmake 配置（openssl 提供 zip 加密；zlib 必需；bzip2 可选）"
  local -a cm=( -DENABLE_LZMA=OFF -DENABLE_ZSTD=OFF )
  [ "${ENABLE_LIBZIP_BZIP2:-0}" = 1 ] && cm+=( -DENABLE_BZIP2=ON ) || cm+=( -DENABLE_BZIP2=OFF )
  [ "${ENABLE_OPENSSL:-0}" = 1 ] && cm+=( -DENABLE_OPENSSL=ON ) || cm+=( -DENABLE_OPENSSL=OFF )
  local rpath="$D/lib;$OPENSSL_D/lib;$ZLIB_D/lib"
  local prefix="$ZLIB_D;$OPENSSL_D;$D"
  if [ -n "$BZIP2_D" ]; then
    rpath="$rpath;$BZIP2_D/lib"
    prefix="$BZIP2_D;$prefix"
  fi
  cmake -S "$SRC" -B "$BDIR" \
    -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_INSTALL_RPATH="$rpath" \
    -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
    -DBUILD_SHARED_LIBS=ON \
    "${cm[@]}" \
    -DBUILD_TOOLS=OFF -DBUILD_REGRESS=OFF -DBUILD_EXAMPLES=OFF -DBUILD_DOC=OFF \
    -DCMAKE_PREFIX_PATH="$prefix" \
    > "$LOGS/libzip-configure.log" 2>&1 \
    || { tail -30 "$LOGS/libzip-configure.log"; die "libzip cmake 配置失败"; }

  log "编译 + 安装"
  cmake --build "$BDIR" -j "$JOBS" > "$LOGS/libzip-make.log" 2>&1 \
    || { tail -30 "$LOGS/libzip-make.log"; die "libzip 编译失败"; }
  cmake --install "$BDIR" >> "$LOGS/libzip-make.log" 2>&1

  stamp_write libzip "$SIG"
  ok "libzip $LIBZIP_VERSION -> $D"
}

recipe_main "$@"
