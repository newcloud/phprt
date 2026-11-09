#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=avif version=AVIF_VERSION needs=aom optional= tools= upstream=github:AOMediaCodec/libavif
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc g++ make cmake tar pkg-config

  local D A SIG TAR SRC
  D="$(dep_dir_of avif)"
  A="$(dep_dir_of aom)"
  SIG="avif=$AVIF_VERSION;aom=$AOM_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok avif "$SIG" && [ -f "$D/lib/libavif.so" ]; then
    ok "libavif 已安装，跳过"; return 0
  fi

  ensure_needs avif
  dep_env avif
  ensure_clean_target avif "$D"

  # GitHub 的 tag 归档 URL 最后一段是 v1.3.0.tar.gz，缓存文件用可读的名字
  TAR="libavif-$AVIF_VERSION.tar.gz"
  fetch "$AVIF_URL" "$DOWNLOADS/$TAR" "${AVIF_SHA256:-}"

  SRC="$SRCDIR/libavif-$AVIF_VERSION"
  if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  log "cmake + make -j$JOBS"
  cmake -S "$SRC" -B "$SRC/build" \
    -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_INSTALL_RPATH="$A/lib;$D/lib" \
    -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DBUILD_SHARED_LIBS=ON \
    -DAVIF_CODEC_AOM=SYSTEM \
    -DAVIF_LIBYUV=OFF \
    -DAVIF_BUILD_APPS=OFF -DAVIF_BUILD_EXAMPLES=OFF -DAVIF_BUILD_TESTS=OFF \
    -DAVIF_BUILD_MAN_PAGES=OFF -DAVIF_ENABLE_WERROR=OFF \
    > "$LOGS/avif-cmake.log" 2>&1 \
    || { tail -30 "$LOGS/avif-cmake.log"; die "libavif cmake 失败"; }
  cmake --build "$SRC/build" --parallel "$JOBS" > "$LOGS/avif-make.log" 2>&1 \
    || { tail -30 "$LOGS/avif-make.log"; die "libavif make 失败"; }
  cmake --install "$SRC/build" >> "$LOGS/avif-make.log" 2>&1

  stamp_write avif "$SIG"
  ok "libavif $AVIF_VERSION -> $D"
}

recipe_main "$@"
