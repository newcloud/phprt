#!/usr/bin/env bash
# =============================================================================
#  libavif（gd 的 AVIF 支持；后端用 libaom，读/写都支持）
#  phprt: switch=ENABLE_GD_AVIF needs=aom
#  安装到: deps/avif-<版本>/
#  可单独执行: lib/modules/deps/avif.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc g++ make cmake tar pkg-config

D="$(dep_dir_of avif)"
A="$(dep_dir_of aom)"
SIG="avif=$AVIF_VERSION;aom=$AOM_VERSION;rpath=1"

if [ "$FORCE" != 1 ] && stamp_ok avif "$SIG" && [ -f "$D/lib/libavif.so" ]; then
  ok "libavif 已安装，跳过"; exit 0
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
  # GitHub 的 tag 归档顶层是 libavif-<版本>/，直接留一层
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
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
