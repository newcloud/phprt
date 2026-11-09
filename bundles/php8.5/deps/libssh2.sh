#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=libssh2 version=LIBSSH2_VERSION needs=openssl,zlib optional= tools= upstream=github:libssh2/libssh2
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  D="$(dep_dir_of libssh2)"
  SIG="libssh2=$LIBSSH2_VERSION;openssl=$OPENSSL_VERSION;zlib=$ZLIB_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok libssh2 "$SIG" && [ -f "$D/lib/libssh2.so" ]; then
    ok "libssh2 已安装，跳过"; return 0
  fi

  ensure_needs libssh2
  dep_env libssh2
  ensure_clean_target libssh2 "$D"

  TAR="$(basename "$LIBSSH2_URL")"
  fetch "$LIBSSH2_URL" "$DOWNLOADS/$TAR" "${LIBSSH2_SHA256:-}"

  SRC="$SRCDIR/libssh2-$LIBSSH2_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  log "configure + make -j$JOBS（crypto=openssl，zlib 压缩）"
  ./configure --prefix="$D" --disable-static \
    --with-crypto=openssl --with-libz \
    > "$LOGS/libssh2-configure.log" 2>&1 \
    || { tail -30 "$LOGS/libssh2-configure.log"; die "libssh2 configure 失败"; }
  make -j"$JOBS" > "$LOGS/libssh2-make.log" 2>&1 \
    || { tail -20 "$LOGS/libssh2-make.log"; die "libssh2 make 失败"; }
  make install >> "$LOGS/libssh2-make.log" 2>&1

  stamp_write libssh2 "$SIG"
  ok "libssh2 $LIBSSH2_VERSION -> $D"
}

recipe_main "$@"
