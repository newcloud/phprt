#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=oniguruma version=ONIGURUMA_VERSION needs= optional= tools= upstream=github:kkos/oniguruma
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc make tar
  lto_on deps && need_cmds gcc-ar gcc-ranlib

  local D SIG TAR SRC
  D="$(dep_dir_of oniguruma)"
  SIG="oniguruma=$ONIGURUMA_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok oniguruma "$SIG" && [ -f "$D/lib/libonig.so" ]; then
    ok "oniguruma 已安装，跳过"; return 0
  fi

  ensure_clean_target oniguruma "$D"

  TAR="$(basename "$ONIGURUMA_URL")"
  fetch "$ONIGURUMA_URL" "$DOWNLOADS/$TAR" "${ONIGURUMA_SHA256:-}"

  SRC="$SRCDIR/onig-$ONIGURUMA_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  local ar_bin ranlib_bin lto_c lto_ld ldflags
  ar_bin="$(dep_ar)"
  ranlib_bin="$(dep_ranlib)"
  lto_c="$(dep_lto_cflags)"
  lto_ld="$(dep_lto_ldflags)"
  ldflags="-Wl,-rpath,$D/lib -Wl,--enable-new-dtags"
  [ -n "$lto_ld" ] && ldflags="$ldflags $lto_ld"
  local -a cfg_env=(LDFLAGS="$ldflags" AR="$ar_bin" RANLIB="$ranlib_bin")
  [ -n "$lto_c" ] && cfg_env+=(CFLAGS="${CFLAGS:--g -O2} $lto_c")
  log "configure + make -j$JOBS"
  env "${cfg_env[@]}" \
    ./configure --prefix="$D" > "$LOGS/oniguruma-configure.log" 2>&1 \
    || { tail -30 "$LOGS/oniguruma-configure.log"; die "oniguruma configure 失败"; }
  make -j"$JOBS" AR="$ar_bin" RANLIB="$ranlib_bin" \
      > "$LOGS/oniguruma-make.log" 2>&1 \
    || { tail -30 "$LOGS/oniguruma-make.log"; die "oniguruma make 失败"; }
  make AR="$ar_bin" RANLIB="$ranlib_bin" install >> "$LOGS/oniguruma-make.log" 2>&1

  [ -f "$D/lib/pkgconfig/oniguruma.pc" ] || die "oniguruma 没装出 oniguruma.pc（PHP mbstring 找不到它）"
  stamp_write oniguruma "$SIG"
  ok "oniguruma $ONIGURUMA_VERSION -> $D"
}

recipe_main "$@"
