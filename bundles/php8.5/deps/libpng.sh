#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=libpng version=LIBPNG_VERSION needs=zlib optional= tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# pnggroup 的 tag 第一页全是 1.7 betas，稳定版在第二页；取两页再过滤预发布
versions() {
  local p
  for p in 1 2; do
    upstream_fetch "github-tags-pnggroup-libpng-p$p" \
      "https://api.github.com/repos/pnggroup/libpng/tags?per_page=100&page=$p"
  done | grep -oE '"name"[[:space:]]*:[[:space:]]*"v[0-9]+\.[0-9]+\.[0-9]+"' |
    sed -E 's/.*"v([0-9.]+)".*/\1/' | sort -u
}

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  D="$(dep_dir_of libpng)"
  SIG="libpng=$LIBPNG_VERSION;zlib=$ZLIB_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok libpng "$SIG" && [ -f "$D/lib/libpng16.so" ]; then
    ok "libpng 已安装，跳过"; return 0
  fi

  ensure_needs libpng
  dep_env libpng
  ensure_clean_target libpng "$D"

  TAR="$(basename "$LIBPNG_URL")"
  fetch "$LIBPNG_URL" "$DOWNLOADS/$TAR" "${LIBPNG_SHA256:-}"

  SRC="$SRCDIR/libpng-$LIBPNG_VERSION"
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
  ./configure --prefix="$D" --disable-static > "$LOGS/libpng-configure.log" 2>&1 \
    || { tail -20 "$LOGS/libpng-configure.log"; die "libpng configure 失败"; }
  make -j"$JOBS" > "$LOGS/libpng-make.log" 2>&1 \
    || { tail -20 "$LOGS/libpng-make.log"; die "libpng make 失败"; }
  make install >> "$LOGS/libpng-make.log" 2>&1

  stamp_write libpng "$SIG"
  ok "libpng $LIBPNG_VERSION -> $D"
}

recipe_main "$@"
