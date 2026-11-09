#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=libsodium version=LIBSODIUM_VERSION needs= optional= tools= upstream=github:jedisct1/libsodium
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  D="$(dep_dir_of libsodium)"
  SIG="libsodium=$LIBSODIUM_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok libsodium "$SIG" && [ -f "$D/lib/libsodium.so" ]; then
    ok "libsodium 已安装，跳过"; return 0
  fi

  ensure_clean_target libsodium "$D"

  TAR="$(basename "$LIBSODIUM_URL")"
  fetch "$LIBSODIUM_URL" "$DOWNLOADS/$TAR" "${LIBSODIUM_SHA256:-}"

  SRC="$SRCDIR/libsodium-$LIBSODIUM_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  log "configure + make -j$JOBS"
  ./configure --prefix="$D" --disable-static > "$LOGS/libsodium-configure.log" 2>&1 \
    || { tail -20 "$LOGS/libsodium-configure.log"; die "libsodium configure 失败"; }
  make -j"$JOBS" > "$LOGS/libsodium-make.log" 2>&1 \
    || { tail -20 "$LOGS/libsodium-make.log"; die "libsodium make 失败"; }
  make install >> "$LOGS/libsodium-make.log" 2>&1

  stamp_write libsodium "$SIG"
  ok "libsodium $LIBSODIUM_VERSION -> $D"
}

recipe_main "$@"
