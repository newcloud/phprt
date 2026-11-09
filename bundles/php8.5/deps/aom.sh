#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=aom version=AOM_VERSION needs= optional= tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# aom-releases 是公开的 GCS 桶，用 JSON API 列对象；网页索引不可列
versions() {
  upstream_fetch gcs-aom-releases \
    "https://storage.googleapis.com/storage/v1/b/aom-releases/o?maxResults=1000&fields=items(name)" |
    grep -oE 'libaom-[0-9]+(\.[0-9]+)+\.tar\.gz' |
    upstream_norm_version | sort -u
}

build() {
  need_cmds gcc g++ make cmake tar

  local D SIG TAR SRC
  local -a ASM_ARGS=()
  D="$(dep_dir_of aom)"
  SIG="aom=$AOM_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok aom "$SIG" && [ -f "$D/lib/libaom.so" ]; then
    ok "libaom 已安装，跳过"; return 0
  fi

  ensure_clean_target aom "$D"

  TAR="$(basename "$AOM_URL")"
  fetch "$AOM_URL" "$DOWNLOADS/$TAR" "${AOM_SHA256:-}"

  SRC="$SRCDIR/libaom-$AOM_VERSION"
  if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  # 没有 nasm/yasm 时 libaom 会拒绝配置，退化成 generic（无 SIMD）
  command -v nasm >/dev/null 2>&1 || command -v yasm >/dev/null 2>&1 || {
    warn "没装 nasm/yasm，libaom 用 generic CPU 编译（会慢一些）"
    ASM_ARGS+=(-DAOM_TARGET_CPU=generic)
  }

  log "cmake + make -j$JOBS（AV1，编码器开着，比较慢）"
  cmake -S "$SRC" -B "$SRC/build" \
    -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DBUILD_SHARED_LIBS=ON \
    -DENABLE_TESTS=OFF -DENABLE_EXAMPLES=OFF -DENABLE_DOCS=OFF -DENABLE_TOOLS=OFF \
    -DCONFIG_AV1_ENCODER=1 -DCONFIG_AV1_DECODER=1 -DCONFIG_MULTITHREAD=1 \
    ${ASM_ARGS[@]+"${ASM_ARGS[@]}"} \
    > "$LOGS/aom-cmake.log" 2>&1 \
    || { tail -30 "$LOGS/aom-cmake.log"; die "libaom cmake 失败"; }
  cmake --build "$SRC/build" --parallel "$JOBS" > "$LOGS/aom-make.log" 2>&1 \
    || { tail -40 "$LOGS/aom-make.log"; die "libaom make 失败"; }
  cmake --install "$SRC/build" >> "$LOGS/aom-make.log" 2>&1

  stamp_write aom "$SIG"
  ok "libaom $AOM_VERSION -> $D"
}

recipe_main "$@"
