#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=libmemcached version=LIBMEMCACHED_VERSION needs= optional= tools= upstream=github:awesomized/libmemcached
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc g++ make cmake tar

  local D SIG TAR SRC BDIR
  D="$(dep_dir_of libmemcached)"
  SIG="libmemcached=$LIBMEMCACHED_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok libmemcached "$SIG" && [ -f "$D/lib/libmemcached.so" ]; then
    ok "libmemcached 已安装，跳过"; return 0
  fi

  ensure_clean_target libmemcached "$D"

  TAR="$(basename "$LIBMEMCACHED_URL")"
  fetch "$LIBMEMCACHED_URL" "$DOWNLOADS/$TAR" "${LIBMEMCACHED_SHA256:-}"

  # GitHub tag 归档顶层是 libmemcached-<版本>/
  SRC="$SRCDIR/libmemcached-$LIBMEMCACHED_VERSION"
  if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  BDIR="$SRCDIR/libmemcached-build-$LIBMEMCACHED_VERSION"
  rm -rf "$BDIR"

  log "cmake + make -j$JOBS（SASL/文档/测试关掉）"
  cmake -S "$SRC" -B "$BDIR" \
    -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DBUILD_SHARED_LIBS=ON \
    -DBUILD_TESTING=OFF -DBUILD_DOCS=OFF -DBUILD_DOCS_MAN=OFF -DBUILD_DOCS_HTML=OFF \
    -DENABLE_SASL=OFF -DENABLE_MEMASLAP=OFF -DENABLE_OPENSSL_CRYPTO=OFF \
    > "$LOGS/libmemcached-cmake.log" 2>&1 \
    || { tail -30 "$LOGS/libmemcached-cmake.log"; die "libmemcached cmake 失败"; }
  cmake --build "$BDIR" --parallel "$JOBS" > "$LOGS/libmemcached-make.log" 2>&1 \
    || { tail -40 "$LOGS/libmemcached-make.log"; die "libmemcached make 失败"; }
  cmake --install "$BDIR" >> "$LOGS/libmemcached-make.log" 2>&1

  stamp_write libmemcached "$SIG"
  ok "libmemcached $LIBMEMCACHED_VERSION -> $D"
}

recipe_main "$@"
