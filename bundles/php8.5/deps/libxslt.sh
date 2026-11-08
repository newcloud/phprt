#!/usr/bin/env bash
# =============================================================================
#  libxslt（xsl 扩展的 XSLT 引擎，依赖 libxml2）
#  phprt: needs=libxml2
#  安装到: deps/libxslt-<版本>/
#  可单独执行: lib/modules/deps/libxslt.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of libxslt)"
SIG="libxslt=${LIBXSLT_VERSION:-};libxml2=${LIBXML2_VERSION:-};rpath=1"

if [ "$FORCE" != 1 ] && stamp_ok libxslt "$SIG" && [ -f "$D/lib/libxslt.so" ]; then
  ok "libxslt 已安装，跳过"; exit 0
fi

ensure_needs libxslt
dep_env libxslt

ensure_clean_target libxslt "$D"

TAR="$(basename "$LIBXSLT_URL")"
fetch "$LIBXSLT_URL" "$DOWNLOADS/$TAR" "${LIBXSLT_SHA256:-}"

SRC="$SRCDIR/libxslt-$LIBXSLT_VERSION"
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
./configure --prefix="$D" --disable-static --with-libxml-prefix="$(dep_dir_of libxml2)" \
  > "$LOGS/libxslt-configure.log" 2>&1 \
  || { tail -20 "$LOGS/libxslt-configure.log"; die "libxslt configure 失败"; }
make -j"$JOBS" > "$LOGS/libxslt-make.log" 2>&1 \
  || { tail -20 "$LOGS/libxslt-make.log"; die "libxslt make 失败"; }
make install >> "$LOGS/libxslt-make.log" 2>&1

stamp_write libxslt "$SIG"
ok "libxslt $LIBXSLT_VERSION -> $D"
