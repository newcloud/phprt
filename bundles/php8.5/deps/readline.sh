#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=readline version=READLINE_VERSION needs=ncurses optional= tools= upstream=none
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# GNU readline：PHP ext/readline 的行编辑后端。
# 终端能力用 ncurses 的 tgetent（--with-curses），不依赖系统的 termcap。
build() {
  need_cmds gcc make tar

  local D SIG TAR SRC
  D="$(dep_dir_of readline)"
  SIG="readline=$READLINE_VERSION;ncurses=$NCURSES_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok readline "$SIG" && [ -f "$D/lib/libreadline.so" ]; then
    ok "GNU readline 已安装，跳过"; return 0
  fi

  ensure_needs readline
  dep_env readline
  ensure_clean_target readline "$D"

  TAR="$(basename "$READLINE_URL")"
  fetch "$READLINE_URL" "$DOWNLOADS/$TAR" "${READLINE_SHA256:-}"

  SRC="$SRCDIR/readline-$READLINE_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  log "configure + make -j$JOBS（termcap 用 deps/ncurses，并把它写进 .so 的 NEEDED）"
  ./configure --prefix="$D" --with-curses --with-shared-termcap-library \
    > "$LOGS/readline-configure.log" 2>&1 \
    || { tail -30 "$LOGS/readline-configure.log"; die "readline configure 失败"; }
  make -j"$JOBS" > "$LOGS/readline-make.log" 2>&1 \
    || { tail -30 "$LOGS/readline-make.log"; die "readline make 失败"; }
  make install >> "$LOGS/readline-make.log" 2>&1

  [ -f "$D/lib/libreadline.so" ] || die "readline 没有产出 $D/lib/libreadline.so"
  stamp_write readline "$SIG"
  ok "GNU readline $READLINE_VERSION -> $D"
}

recipe_main "$@"
