#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=jpeg version=JPEG_VERSION needs= optional= tools= upstream=github:libjpeg-turbo/libjpeg-turbo
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc make cmake tar

  local D SIG TAR SRC
  D="$(dep_dir_of jpeg)"
  SIG="jpeg=$JPEG_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok jpeg "$SIG" && [ -f "$D/lib/libjpeg.so" ]; then
    ok "libjpeg-turbo 已安装，跳过"; return 0
  fi

  ensure_clean_target jpeg "$D"

  TAR="$(basename "$JPEG_URL")"
  fetch "$JPEG_URL" "$DOWNLOADS/$TAR" "${JPEG_SHA256:-}"

  SRC="$SRCDIR/libjpeg-turbo-$JPEG_VERSION"
  if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  # CMakeCache 会记住依赖的绝对路径；依赖升级换目录后必须重新生成
  rm -rf "$SRC/build"

  log "cmake + make -j$JOBS（没有 nasm，关 SIMD）"
  cmake -S "$SRC" -B "$SRC/build" \
    -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_INSTALL_INCLUDEDIR=include \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DENABLE_SHARED=ON -DENABLE_STATIC=OFF \
    -DWITH_SIMD=OFF -DWITH_TURBOJPEG=OFF \
    > "$LOGS/jpeg-cmake.log" 2>&1 \
    || { tail -30 "$LOGS/jpeg-cmake.log"; die "libjpeg-turbo cmake 失败"; }
  cmake --build "$SRC/build" --parallel "$JOBS" > "$LOGS/jpeg-make.log" 2>&1 \
    || { tail -30 "$LOGS/jpeg-make.log"; die "libjpeg-turbo make 失败"; }
  cmake --install "$SRC/build" >> "$LOGS/jpeg-make.log" 2>&1

  stamp_write jpeg "$SIG"
  ok "libjpeg-turbo $JPEG_VERSION -> $D"
}

recipe_main "$@"
