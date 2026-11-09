#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=webp version=WEBP_VERSION needs= optional= tools= upstream=github:webmproject/libwebp
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc make cmake tar

  local D SIG TAR SRC
  D="$(dep_dir_of webp)"
  SIG="webp=$WEBP_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok webp "$SIG" && [ -f "$D/lib/libwebp.so" ]; then
    ok "libwebp 已安装，跳过"; return 0
  fi

  ensure_clean_target webp "$D"

  TAR="$(basename "$WEBP_URL")"
  fetch "$WEBP_URL" "$DOWNLOADS/$TAR" "${WEBP_SHA256:-}"

  SRC="$SRCDIR/libwebp-$WEBP_VERSION"
  if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  log "cmake + make -j$JOBS（只编库，不编 cwebp/dwebp 这些工具）"
  cmake -S "$SRC" -B "$SRC/build" \
    -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DBUILD_SHARED_LIBS=ON \
    -DWEBP_BUILD_ANIM_UTILS=OFF -DWEBP_BUILD_CWEBP=OFF -DWEBP_BUILD_DWEBP=OFF \
    -DWEBP_BUILD_GIF2WEBP=OFF -DWEBP_BUILD_IMG2WEBP=OFF -DWEBP_BUILD_VWEBP=OFF \
    -DWEBP_BUILD_WEBPINFO=OFF -DWEBP_BUILD_WEBPMUX=OFF -DWEBP_BUILD_EXTRAS=OFF \
    > "$LOGS/webp-cmake.log" 2>&1 \
    || { tail -30 "$LOGS/webp-cmake.log"; die "libwebp cmake 失败"; }
  cmake --build "$SRC/build" --parallel "$JOBS" > "$LOGS/webp-make.log" 2>&1 \
    || { tail -30 "$LOGS/webp-make.log"; die "libwebp make 失败"; }
  cmake --install "$SRC/build" >> "$LOGS/webp-make.log" 2>&1

  stamp_write webp "$SIG"
  ok "libwebp $WEBP_VERSION -> $D"
}

recipe_main "$@"
