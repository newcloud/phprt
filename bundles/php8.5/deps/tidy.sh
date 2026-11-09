#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=tidy version=TIDY_VERSION needs= optional= tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# 上游 tag 里 5.9.x 都带 -next（开发版），只保留纯数字版本
versions() {
  upstream_fetch github-tags-htacg-tidy-html5 \
    "https://api.github.com/repos/htacg/tidy-html5/tags?per_page=100" |
    grep -oE '"name"[[:space:]]*:[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+"' |
    sed -E 's/.*"([0-9.]+)".*/\1/' | sort -u
}

build() {
  need_cmds gcc make cmake tar

  local D SIG TAR SRC
  D="$(dep_dir_of tidy)"
  SIG="tidy=$TIDY_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok tidy "$SIG" && [ -f "$D/lib/libtidy.so" ]; then
    ok "tidy 已安装，跳过"; return 0
  fi

  ensure_clean_target tidy "$D"

  TAR="$(basename "$TIDY_URL")"
  fetch "$TIDY_URL" "$DOWNLOADS/$TAR" "${TIDY_SHA256:-}"

  SRC="$SRCDIR/tidy-html5-$TIDY_VERSION"
  if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  # CMakeCache 会记住依赖的绝对路径；依赖升级换目录后必须重新生成
  rm -rf "$SRC/build"

  log "cmake + make -j$JOBS"
  # tidy-html5 5.8.0 的 CMakeLists 还写着 cmake_minimum_required(<3.5)，
  # CMake 4 已移除兼容；显式给最低策略版本。
  cmake -S "$SRC" -B "$SRC/build" \
    -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
    -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DBUILD_SHARED_LIB=ON \
    > "$LOGS/tidy-cmake.log" 2>&1 \
    || { tail -30 "$LOGS/tidy-cmake.log"; die "tidy cmake 失败"; }
  cmake --build "$SRC/build" --parallel "$JOBS" > "$LOGS/tidy-make.log" 2>&1 \
    || { tail -30 "$LOGS/tidy-make.log"; die "tidy make 失败"; }
  cmake --install "$SRC/build" >> "$LOGS/tidy-make.log" 2>&1

  stamp_write tidy "$SIG"
  ok "tidy $TIDY_VERSION -> $D"
}

recipe_main "$@"
