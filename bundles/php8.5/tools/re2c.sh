#!/usr/bin/env bash
# phprt: contract=v2 kind=tool name=re2c version=RE2C_VERSION tools= upstream=github:skvadrik/re2c
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc g++ make tar

  local D SIG TAR SRC
  D="$(dep_dir_of re2c)"
  SIG="re2c=$RE2C_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok re2c "$SIG" && [ -x "$D/bin/re2c" ]; then
    ok "re2c 已安装，跳过"; return 0
  fi

  ensure_clean_target re2c "$D"

  TAR="$(basename "$RE2C_URL")"
  fetch "$RE2C_URL" "$DOWNLOADS/$TAR" "${RE2C_SHA256:-}"

  SRC="$SRCDIR/re2c-$RE2C_VERSION"
  if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  log "configure + make -j$JOBS"
  ./configure --prefix="$D" --disable-static > "$LOGS/re2c-configure.log" 2>&1 \
    || { tail -20 "$LOGS/re2c-configure.log"; die "re2c configure 失败"; }
  make -j"$JOBS" > "$LOGS/re2c-make.log" 2>&1 \
    || { tail -20 "$LOGS/re2c-make.log"; die "re2c make 失败"; }
  make install >> "$LOGS/re2c-make.log" 2>&1

  # re2c 默认带调试符号，整目录近 200M，装完 strip
  local b
  for b in "$D"/bin/*; do
    [ -f "$b" ] && strip "$b" 2>/dev/null || true
  done

  stamp_write re2c "$SIG"
  ok "re2c $RE2C_VERSION -> $D/bin/re2c"
}

recipe_main "$@"
