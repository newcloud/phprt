#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=rabbitmq-c version=RABBITMQ_C_VERSION needs=openssl optional= tools= upstream=github:alanxz/rabbitmq-c
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc make cmake tar

  local D SIG TAR SRC BDIR
  D="$(dep_dir_of rabbitmq-c)"
  SIG="rabbitmq-c=$RABBITMQ_C_VERSION;openssl=$OPENSSL_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok rabbitmq-c "$SIG" && [ -f "$D/lib/librabbitmq.so" ]; then
    ok "rabbitmq-c 已安装，跳过"; return 0
  fi

  ensure_needs rabbitmq-c
  dep_env rabbitmq-c
  ensure_clean_target rabbitmq-c "$D"

  TAR="$(basename "$RABBITMQ_C_URL")"
  fetch "$RABBITMQ_C_URL" "$DOWNLOADS/$TAR" "${RABBITMQ_C_SHA256:-}"

  SRC="$SRCDIR/rabbitmq-c-$RABBITMQ_C_VERSION"
  if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  BDIR="$SRCDIR/rabbitmq-c-build-$RABBITMQ_C_VERSION"
  rm -rf "$BDIR"

  log "cmake + make -j$JOBS（工具/示例/测试关掉，SSL 用树内 openssl）"
  cmake -S "$SRC" -B "$BDIR" \
    -DCMAKE_INSTALL_PREFIX="$D" \
    -DCMAKE_INSTALL_LIBDIR=lib \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DBUILD_SHARED_LIBS=ON \
    -DBUILD_EXAMPLES=OFF -DBUILD_TESTS=OFF -DBUILD_TOOLS=OFF -DBUILD_DOCS=OFF \
    -DENABLE_SSL_SUPPORT=ON \
    -DOPENSSL_ROOT_DIR="$(dep_dir_of openssl)" \
    > "$LOGS/rabbitmq-c-cmake.log" 2>&1 \
    || { tail -30 "$LOGS/rabbitmq-c-cmake.log"; die "rabbitmq-c cmake 失败"; }
  cmake --build "$BDIR" --parallel "$JOBS" > "$LOGS/rabbitmq-c-make.log" 2>&1 \
    || { tail -30 "$LOGS/rabbitmq-c-make.log"; die "rabbitmq-c make 失败"; }
  cmake --install "$BDIR" >> "$LOGS/rabbitmq-c-make.log" 2>&1

  stamp_write rabbitmq-c "$SIG"
  ok "rabbitmq-c $RABBITMQ_C_VERSION -> $D"
}

recipe_main "$@"
