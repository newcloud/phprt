#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=gmp version=GMP_VERSION needs= optional= tools=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

versions() {
  upstream_versions_listing https://gmplib.org/download/gmp/ \
    'gmp-[0-9]+(\.[0-9]+)+\.tar\.(xz|gz)'
}

build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  D="$(dep_dir_of gmp)"
  SIG="gmp=$GMP_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok gmp "$SIG" && [ -f "$D/lib/libgmp.so" ]; then
    ok "gmp 已安装，跳过"; return 0
  fi

  ensure_clean_target gmp "$D"

  TAR="$(basename "$GMP_URL")"
  fetch "$GMP_URL" "$DOWNLOADS/$TAR" "${GMP_SHA256:-}"

  SRC="$SRCDIR/gmp-$GMP_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  # GMP 6.3.0 的编译器探测程序用了隐式函数声明；GCC 15 默认 gnu23 会把它当错误，
  # configure 因此报 "could not find a working compiler"。固定到 gnu17。
  export CC="${CC:-gcc} -std=gnu17"
  log "configure + make -j$JOBS"
  ./configure --prefix="$D" --enable-shared --disable-static \
    > "$LOGS/gmp-configure.log" 2>&1 \
    || { tail -20 "$LOGS/gmp-configure.log"; die "gmp configure 失败"; }
  make -j"$JOBS" > "$LOGS/gmp-make.log" 2>&1 \
    || { tail -20 "$LOGS/gmp-make.log"; die "gmp make 失败"; }
  make install >> "$LOGS/gmp-make.log" 2>&1

  stamp_write gmp "$SIG"
  ok "gmp $GMP_VERSION -> $D"
}

recipe_main "$@"
