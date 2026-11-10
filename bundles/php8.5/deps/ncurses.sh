#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=ncurses version=NCURSES_VERSION needs= optional= tools= upstream=none note=ncurses-widec+ncurses-cxx
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# ncurses 只作为 readline 的 termcap 来源；不开 widec，
# 因为 PHP 的 ext/readline 用 AC_CHECK_LIB([ncurses], [tgetent]) 只认 libncurses。
build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  D="$(dep_dir_of ncurses)"
  SIG="ncurses=$NCURSES_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok ncurses "$SIG" && [ -f "$D/lib/libncurses.so" ]; then
    ok "ncurses 已安装，跳过"; return 0
  fi

  ensure_clean_target ncurses "$D"

  TAR="$(basename "$NCURSES_URL")"
  fetch "$NCURSES_URL" "$DOWNLOADS/$TAR" "${NCURSES_SHA256:-}"

  SRC="$SRCDIR/ncurses-$NCURSES_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  log "configure + make -j$JOBS（窄字符/C++ 绑定/ada/man 关掉，只要 libncurses）"
  ./configure --prefix="$D" \
    --with-shared --with-normal --without-termlib \
    --disable-widec --without-debug --without-cxx --without-cxx-binding \
    --without-ada --without-manpages \
    --enable-overwrite \
    > "$LOGS/ncurses-configure.log" 2>&1 \
    || { tail -30 "$LOGS/ncurses-configure.log"; die "ncurses configure 失败"; }
  make -j"$JOBS" > "$LOGS/ncurses-make.log" 2>&1 \
    || { tail -30 "$LOGS/ncurses-make.log"; die "ncurses make 失败"; }
  make install >> "$LOGS/ncurses-make.log" 2>&1

  [ -f "$D/lib/libncurses.so" ] || die "ncurses 没有产出 $D/lib/libncurses.so"
  stamp_write ncurses "$SIG"
  ok "ncurses $NCURSES_VERSION -> $D"
}

recipe_main "$@"
