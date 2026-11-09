#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=zlib version=ZLIB_VERSION needs= optional= tools= wheel=auto
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  D="$(dep_dir_of zlib)"
  SIG="zlib=$ZLIB_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok zlib "$SIG" && [ -f "$D/lib/libz.so" ]; then
    ok "zlib 已安装，跳过"; return 0
  fi

  ensure_clean_target zlib "$D"

  TAR="$(basename "$ZLIB_URL")"
  fetch "$ZLIB_URL" "$DOWNLOADS/$TAR" "${ZLIB_SHA256:-}"

  SRC="$SRCDIR/zlib-$ZLIB_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
    [ -d "$SRC" ] || die "解压后未找到 $SRC"
  fi

  cd "$SRC"
  log "configure + make -j$JOBS"
  ./configure --prefix="$D" > "$LOGS/zlib-configure.log" 2>&1 \
    || { tail -20 "$LOGS/zlib-configure.log"; die "zlib configure 失败"; }
  make -j"$JOBS" > "$LOGS/zlib-make.log" 2>&1 \
    || { tail -20 "$LOGS/zlib-make.log"; die "zlib make 失败"; }
  make install >> "$LOGS/zlib-make.log" 2>&1

  stamp_write zlib "$SIG"
  ok "zlib $ZLIB_VERSION -> $D"
}

# 上游版本：zlib.net 的 fossils 目录列出了历史版本
versions() {
  upstream_versions_listing https://zlib.net/fossils/ \
    'zlib-[0-9]+(\.[0-9]+)+\.tar\.(gz|xz)'
}

recipe_main "$@"
