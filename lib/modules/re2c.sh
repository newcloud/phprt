#!/usr/bin/env bash
# =============================================================================
#  构建工具：re2c（PHP 的扫描器生成器）
#  只在「从 git 归档/分支构建」（源码包没有 configure，需要 buildconf）时才用到；
#  官方 release 的 tar.xz 已经带 configure，不需要它。
#  安装到: tools/re2c-<版本>/
#  由 php.sh 自动调用，不作为开关暴露
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
load_config

need_cmds gcc g++ make tar

D="$(dep_dir_of re2c)"
SIG="re2c=$RE2C_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok re2c "$SIG" && [ -x "$D/bin/re2c" ]; then
  ok "re2c 已安装，跳过"; exit 0
fi

ensure_clean_target re2c "$D"

TAR="$(basename "$RE2C_URL")"
fetch "$RE2C_URL" "$DOWNLOADS/$TAR" "${RE2C_SHA256:-}"

SRC="$SRCDIR/re2c-$RE2C_VERSION"
if [ "$REBUILD" != 1 ] || [ ! -d "$SRC" ]; then
  rm -rf "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

cd "$SRC"
log "configure + make -j$JOBS"
./configure --prefix="$D" --disable-static > "$LOGS/re2c-configure.log" 2>&1 \
  || { tail -20 "$LOGS/re2c-configure.log"; die "re2c configure 失败"; }
make -j"$JOBS" > "$LOGS/re2c-make.log" 2>&1 \
  || { tail -20 "$LOGS/re2c-make.log"; die "re2c make 失败"; }
make install >> "$LOGS/re2c-make.log" 2>&1

# re2c 默认带调试符号，11 个可执行文件加起来近 200M，装完 strip 一下
for _b in "$D"/bin/*; do
  [ -f "$_b" ] && strip "$_b" 2>/dev/null || true
done

stamp_write re2c "$SIG"
ok "re2c $RE2C_VERSION -> $D/bin/re2c"
