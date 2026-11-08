#!/usr/bin/env bash
# =============================================================================
#  bzip2（bz2 扩展的压缩库）
#  phprt: needs=
#  安装到: deps/bzip2-<版本>/
#  可单独执行: lib/modules/deps/bzip2.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of bzip2)"
SIG="bzip2=${BZIP2_VERSION:-};rpath=1"

if [ "$FORCE" != 1 ] && stamp_ok bzip2 "$SIG" && [ -f "$D/lib/libbz2.so" ]; then
  ok "bzip2 已安装，跳过"; exit 0
fi

ensure_clean_target bzip2 "$D"

TAR="$(basename "$BZIP2_URL")"
fetch "$BZIP2_URL" "$DOWNLOADS/$TAR" "${BZIP2_SHA256:-}"

SRC="$SRCDIR/bzip2-$BZIP2_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

cd "$SRC"
# bzip2 无 configure；默认 all 只产静态库，共享库要 Makefile-libbz2_so
#（产物 libbz2.so.1.0.8，soname libbz2.so.1.0）
log "make -f Makefile-libbz2_so -j$JOBS"
make -f Makefile-libbz2_so -j"$JOBS" > "$LOGS/bzip2-make.log" 2>&1 \
  || { tail -20 "$LOGS/bzip2-make.log"; die "bzip2 make 失败"; }

log "安装（bzip2 的 install 目标不装库和头文件，手动 cp）"
mkdir -p "$D/lib" "$D/include"
cp -a libbz2.so.1.0.8 "$D/lib/"
ln -sfn libbz2.so.1.0.8 "$D/lib/libbz2.so.1.0"
ln -sfn libbz2.so.1.0.8 "$D/lib/libbz2.so"
cp -a bzlib.h "$D/include/"

stamp_write bzip2 "$SIG"
ok "bzip2 $BZIP2_VERSION -> $D"
