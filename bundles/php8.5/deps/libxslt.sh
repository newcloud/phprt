#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=libxslt version=LIBXSLT_VERSION needs=libxml2 optional= tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

versions() {
  upstream_versions_listing https://download.gnome.org/sources/libxslt/1.1/ \
    'libxslt-[0-9]+(\.[0-9]+)+\.tar\.xz'
}

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  D="$(dep_dir_of libxslt)"
  SIG="libxslt=$LIBXSLT_VERSION;libxml2=$LIBXML2_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok libxslt "$SIG" && [ -f "$D/lib/libxslt.so" ]; then
    ok "libxslt 已安装，跳过"; return 0
  fi

  ensure_needs libxslt
  dep_env libxslt
  ensure_clean_target libxslt "$D"

  TAR="$(basename "$LIBXSLT_URL")"
  fetch "$LIBXSLT_URL" "$DOWNLOADS/$TAR" "${LIBXSLT_SHA256:-}"

  SRC="$SRCDIR/libxslt-$LIBXSLT_VERSION"
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
  ./configure --prefix="$D" --disable-static --with-libxml-prefix="$(dep_dir_of libxml2)" \
    > "$LOGS/libxslt-configure.log" 2>&1 \
    || { tail -20 "$LOGS/libxslt-configure.log"; die "libxslt configure 失败"; }
  make -j"$JOBS" > "$LOGS/libxslt-make.log" 2>&1 \
    || { tail -20 "$LOGS/libxslt-make.log"; die "libxslt make 失败"; }
  make install >> "$LOGS/libxslt-make.log" 2>&1

  stamp_write libxslt "$SIG"
  ok "libxslt $LIBXSLT_VERSION -> $D"
}

recipe_main "$@"
