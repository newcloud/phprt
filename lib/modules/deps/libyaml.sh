#!/usr/bin/env bash
# =============================================================================
#  libyaml（PHP yaml 扩展的依赖）
#  安装到: deps/libyaml-<版本>/
#  可单独执行: lib/modules/deps/libyaml.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of libyaml)"
SIG="libyaml=$LIBYAML_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok libyaml "$SIG" && [ -f "$D/lib/libyaml.so" ]; then
  ok "libyaml 已安装，跳过"; exit 0
fi

ensure_clean_target libyaml "$D"

TAR="$(basename "$LIBYAML_URL")"
fetch "$LIBYAML_URL" "$DOWNLOADS/$TAR" "${LIBYAML_SHA256:-}"

SRC="$SRCDIR/yaml-$LIBYAML_VERSION"
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
./configure --prefix="$D" --disable-static > "$LOGS/libyaml-configure.log" 2>&1 \
  || { tail -20 "$LOGS/libyaml-configure.log"; die "libyaml configure 失败"; }
make -j"$JOBS" > "$LOGS/libyaml-make.log" 2>&1 \
  || { tail -20 "$LOGS/libyaml-make.log"; die "libyaml make 失败"; }
make install >> "$LOGS/libyaml-make.log" 2>&1

stamp_write libyaml "$SIG"
ok "libyaml $LIBYAML_VERSION -> $D"
