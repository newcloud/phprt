#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=librdkafka version=LIBRDKAFKA_VERSION needs=openssl,zlib optional= tools= upstream=github:confluentinc/librdkafka
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc g++ make tar

  local D SIG TAR SRC
  D="$(dep_dir_of librdkafka)"
  SIG="librdkafka=$LIBRDKAFKA_VERSION;openssl=$OPENSSL_VERSION;zlib=$ZLIB_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok librdkafka "$SIG" && [ -f "$D/lib/librdkafka.so" ]; then
    ok "librdkafka 已安装，跳过"; return 0
  fi

  ensure_needs librdkafka
  dep_env librdkafka
  ensure_clean_target librdkafka "$D"

  TAR="$(basename "$LIBRDKAFKA_URL")"
  fetch "$LIBRDKAFKA_URL" "$DOWNLOADS/$TAR" "${LIBRDKAFKA_SHA256:-}"

  SRC="$SRCDIR/librdkafka-$LIBRDKAFKA_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  log "configure + make -j$JOBS（只留 ssl + zlib）"
  # mklove 的 configure 没有 --disable-static/--disable-tests；
  # 默认就只产出共享库（--enable-static 才是额外要静态库）
  ./configure --prefix="$D" \
    --enable-ssl --enable-zlib \
    --disable-gssapi --disable-sasl --disable-zstd --disable-curl \
    > "$LOGS/librdkafka-configure.log" 2>&1 \
    || { tail -30 "$LOGS/librdkafka-configure.log"; die "librdkafka configure 失败"; }
  make -j"$JOBS" > "$LOGS/librdkafka-make.log" 2>&1 \
    || { tail -40 "$LOGS/librdkafka-make.log"; die "librdkafka make 失败"; }
  make install >> "$LOGS/librdkafka-make.log" 2>&1

  stamp_write librdkafka "$SIG"
  ok "librdkafka $LIBRDKAFKA_VERSION -> $D"
}

recipe_main "$@"
