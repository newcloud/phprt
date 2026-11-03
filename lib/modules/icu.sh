#!/usr/bin/env bash
# =============================================================================
#  ICU（PHP intl 扩展的依赖）
#  注意：ICU4C 是 C++ 项目，必须有 C++ 编译器（g++ / cc1plus + libstdc++ 头文件）
#  安装到: deps/icu-<版本>/
#  可单独执行: lib/modules/icu.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
load_config

need_cmds gcc make tar
need_cmds g++ || die "ICU 需要 C++ 编译器（装上 gcc-c++ / libstdc++-devel 再来）"

D="$(dep_dir_of icu)"
SIG="icu=$ICU_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok icu "$SIG" && [ -e "$D/lib/libicuuc.so" ]; then
  ok "ICU 已安装，跳过"; exit 0
fi

ensure_clean_target icu "$D"

TAR="$(basename "$ICU_URL")"
fetch "$ICU_URL" "$DOWNLOADS/$TAR" "${ICU_SHA256:-}"

SRC="$SRCDIR/icu-$ICU_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC/source" 2>/dev/null && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

cd "$SRC/source" 2>/dev/null || cd "$SRC/icu/source" 2>/dev/null \
  || die "ICU 源码里找不到 source/ 目录"

log "configure + make -j$JOBS（ICU 编译较久，3-5 分钟）"
./configure --prefix="$D" \
  --disable-samples --disable-tests --disable-extras \
  --enable-shared --disable-static \
  --with-data-packaging=library \
  > "$LOGS/icu-configure.log" 2>&1 \
  || { tail -30 "$LOGS/icu-configure.log"; die "ICU configure 失败"; }
make -j"$JOBS" > "$LOGS/icu-make.log" 2>&1 \
  || { tail -40 "$LOGS/icu-make.log"; die "ICU make 失败"; }
make install >> "$LOGS/icu-make.log" 2>&1

stamp_write icu "$SIG"
ok "ICU $ICU_VERSION -> $D"
