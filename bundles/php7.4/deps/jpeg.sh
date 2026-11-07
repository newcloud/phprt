#!/usr/bin/env bash
# =============================================================================
#  libjpeg-turbo（gd 的 JPEG 支持）
#  用 cmake 构建；系统没有 nasm，所以关掉 SIMD
#  安装到: deps/jpeg-<版本>/
#  可单独执行: lib/modules/deps/jpeg.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make cmake tar

D="$(dep_dir_of jpeg)"
SIG="jpeg=$JPEG_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok jpeg "$SIG" && [ -f "$D/lib/libjpeg.so" ]; then
  ok "libjpeg-turbo 已安装，跳过"; exit 0
fi

ensure_clean_target jpeg "$D"

TAR="$(basename "$JPEG_URL")"
fetch "$JPEG_URL" "$DOWNLOADS/$TAR" "${JPEG_SHA256:-}"

SRC="$SRCDIR/libjpeg-turbo-$JPEG_VERSION"
if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
  rm -rf "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

log "cmake + make -j$JOBS"
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
