#!/usr/bin/env bash
# =============================================================================
#  构建工具：autoconf（只在编译 PECL 扩展如 redis 时需要，不是运行时依赖）
#  安装到: tools/autoconf-<版本>/
#  不作为开关暴露；由 redis.sh 自动调用
#  可单独执行: lib/modules/tools/autoconf.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make perl m4 tar

D="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
SIG="autoconf=$AUTOCONF_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok autoconf "$SIG" && [ -x "$D/bin/autoconf" ]; then
  ok "autoconf 已安装，跳过"; exit 0
fi

if [ ! -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ]; then
  log "autoconf 需要 perl 核心模块，先补齐"
  bash "$BUNDLE/tools/perlshim.sh"
fi

ensure_clean_target autoconf "$D"

TAR="$(basename "$AUTOCONF_URL")"
fetch "$AUTOCONF_URL" "$DOWNLOADS/$TAR" "${AUTOCONF_SHA256:-}"

SRC="$SRCDIR/autoconf-$AUTOCONF_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

# 有的精简系统缺 perl 核心模块（File::Compare / File::Copy 之类），版本库里自带一份
SHIM_SRC="$VENDOR/perl"
[ -f "$SHIM_SRC/File/Compare.pm" ] || die "缺少 $SHIM_SRC/File/Compare.pm"

cd "$SRC"
export PERL5LIB="$SHIM_SRC:$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"

log "configure + make -j$JOBS"
./configure --prefix="$D" > "$LOGS/autoconf-configure.log" 2>&1 \
  || { tail -20 "$LOGS/autoconf-configure.log"; die "autoconf configure 失败"; }
make -j"$JOBS" > "$LOGS/autoconf-make.log" 2>&1 \
  || { tail -20 "$LOGS/autoconf-make.log"; die "autoconf make 失败"; }
make install >> "$LOGS/autoconf-make.log" 2>&1

# 装好的命令用包装脚本补 PERL5LIB（只影响 autoconf 系列，不动系统 perl）。
# 包装脚本按自身位置找同目录的 .real 和 perl 模块目录，所以整个安装根搬走也有效。
mkdir -p "$D/share/perl5/File"
cp -f "$SHIM_SRC/File/Compare.pm" "$SHIM_SRC/File/Copy.pm" "$D/share/perl5/File/"
for s in autoconf autoheader autoreconf autom4te autoscan autoupdate ifnames; do
  [ -f "$D/bin/$s" ] || continue
  grep -q 'phprt-perl-shim' "$D/bin/$s" 2>/dev/null && continue
  mv -f "$D/bin/$s" "$D/bin/$s.real"
  cat > "$D/bin/$s" <<'WRAP'
#!/bin/sh
# phprt-perl-shim: 补上系统 perl 缺失的纯 perl 模块（File::Compare / File::Copy …）
here="$(dirname "$(readlink -f "$0")")"
PERL5LIB="$here/../share/perl5:$here/../../perl5core-@PERL_SHIM_VERSION@/share/perl5core${PERL5LIB:+:$PERL5LIB}"
export PERL5LIB
exec "$here/$(basename "$0").real" "$@"
WRAP
  sed -i "s|@PERL_SHIM_VERSION@|$PERL_SHIM_VERSION|" "$D/bin/$s"
  chmod +x "$D/bin/$s"
done

"$D/bin/autoconf" --version > /dev/null 2>&1 || die "autoconf 装好了但跑不起来"

stamp_write autoconf "$SIG"
ok "autoconf $AUTOCONF_VERSION -> $D/bin/autoconf"
