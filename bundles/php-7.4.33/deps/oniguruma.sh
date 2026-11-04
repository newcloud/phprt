#!/usr/bin/env bash
# =============================================================================
#  oniguruma（PHP mbstring 的正则实现；通过 pkg-config 名 oniguruma 被发现）
#  安装到: deps/oniguruma-<版本>/
#  可单独执行: lib/modules/deps/oniguruma.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of oniguruma)"
SIG="oniguruma=$ONIGURUMA_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok oniguruma "$SIG" && [ -f "$D/lib/libonig.so" ]; then
  ok "oniguruma 已安装，跳过"; exit 0
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
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

cd "$SRC"
log "configure + make -j$JOBS"
LDFLAGS="-Wl,-rpath,$D/lib -Wl,--enable-new-dtags" \
  ./configure --prefix="$D" > "$LOGS/oniguruma-configure.log" 2>&1 \
  || { tail -30 "$LOGS/oniguruma-configure.log"; die "oniguruma configure 失败"; }
make -j"$JOBS" > "$LOGS/oniguruma-make.log" 2>&1 \
  || { tail -30 "$LOGS/oniguruma-make.log"; die "oniguruma make 失败"; }
make install >> "$LOGS/oniguruma-make.log" 2>&1

[ -f "$D/lib/pkgconfig/oniguruma.pc" ] || die "oniguruma 没装出 oniguruma.pc（PHP mbstring 找不到它）"
stamp_write oniguruma "$SIG"
ok "oniguruma $ONIGURUMA_VERSION -> $D"
