#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=freetype version=FREETYPE_VERSION needs=zlib,libpng optional= tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

versions() {
  upstream_versions_listing https://download.savannah.gnu.org/releases/freetype/ \
    'freetype-[0-9]+(\.[0-9]+)+\.tar\.(gz|xz)'
}

build() {
  need_cmds gcc make tar pkg-config

  local D SIG TAR SRC
  D="$(dep_dir_of freetype)"
  SIG="freetype=$FREETYPE_VERSION;zlib=$ZLIB_VERSION;libpng=$LIBPNG_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok freetype "$SIG" && [ -f "$D/lib/libfreetype.so" ]; then
    ok "FreeType 已安装，跳过"; return 0
  fi

  ensure_needs freetype
  dep_env freetype
  ensure_clean_target freetype "$D"

  TAR="$(basename "$FREETYPE_URL")"
  fetch "$FREETYPE_URL" "$DOWNLOADS/$TAR" "${FREETYPE_SHA256:-}"

  SRC="$SRCDIR/freetype-$FREETYPE_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  log "configure + make -j$JOBS（harfbuzz/brotli/bzip2 关掉，只用 zlib+png）"
  ./configure --prefix="$D" \
    --with-zlib=yes --with-png=yes \
    --with-harfbuzz=no --with-brotli=no --with-bzip2=no \
    > "$LOGS/freetype-configure.log" 2>&1 \
    || { tail -30 "$LOGS/freetype-configure.log"; die "freetype configure 失败"; }
  make -j"$JOBS" > "$LOGS/freetype-make.log" 2>&1 \
    || { tail -30 "$LOGS/freetype-make.log"; die "freetype make 失败"; }
  make install >> "$LOGS/freetype-make.log" 2>&1

  stamp_write freetype "$SIG"
  ok "FreeType $FREETYPE_VERSION -> $D"
}

recipe_main "$@"
