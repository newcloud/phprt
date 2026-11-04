#!/usr/bin/env bash
# =============================================================================
#  perl 核心模块补充包（构建期工具用，不是运行时依赖）
#  安装到: tools/perl5core-<perl 版本>/share/perl5core/
#
#  有些精简系统只装了 perl 解释器，没有 perl-core：构建期 openssl/autoconf
#  需要的 IPC::Cmd、FindBin、File::Compare 等纯 perl 模块会缺失。这里从官方
#  perl 源码包里取出「不含 XS 的发行版」，需要的地方通过 PERL5LIB 引用，
#  不动系统 perl。
#
#  可单独执行: lib/modules/tools/perlshim.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds tar xz perl

ROOT_D="$(dep_dir_of perl5core)"
DEST="$ROOT_D/share/perl5core"
SYSPERL="$(perl -e 'print $^V' 2>/dev/null || echo unknown)"
SIG="perlshim=$PERL_SHIM_VERSION;sysperl=$SYSPERL"

if [ "$FORCE" != 1 ] && stamp_ok perlshim "$SIG" && [ -f "$DEST/IPC/Cmd.pm" ]; then
  ok "perl 核心模块补充包已就绪，跳过"; exit 0
fi

ensure_clean_target perlshim "$ROOT_D"

TAR="$(basename "$PERL_SHIM_URL")"
fetch "$PERL_SHIM_URL" "$DOWNLOADS/$TAR" "${PERL_SHIM_SHA256:-}"

SRC="$SRCDIR/perl-$PERL_SHIM_VERSION"
if [ ! -d "$SRC/lib" ]; then
  rm -rf "$SRC"
  log "解压 $TAR（只取纯 perl 模块，不编译 perl）"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC/lib" ] || die "解压后未找到 $SRC/lib"
fi

rm -rf "$DEST"; mkdir -p "$DEST"

# 1) 根 lib/ ：perl 官方归类为「纯 perl 核心」的部分
cp -a "$SRC/lib/." "$DEST/"

# 2) dist/ 与 cpan/ 里不含 XS/C 源码的发行版（IPC-Cmd、FindBin、Params-Check…）
added=0
for d in "$SRC"/dist/* "$SRC"/cpan/* "$SRC"/ext/*; do
  [ -d "$d/lib" ] || continue
  if find "$d" -name '*.xs' | grep -q .; then continue; fi   # 带 XS 的发行版跳过（需要编译）
  cp -a "$d/lib/." "$DEST/"
  added=$((added + 1))
done

log "验证关键模块可加载"
PERL5LIB="$DEST" perl -MIPC::Cmd -MFindBin -MFile::Compare -MFile::Copy \
  -e 'print "  IPC::Cmd / FindBin / File::Compare / File::Copy 均可加载\n"' \
  || die "perl 模块补充包不完整"

stamp_write perlshim "$SIG"
ok "perl 核心模块 -> $DEST（纯 perl 发行版 $added 个 + lib/）"
