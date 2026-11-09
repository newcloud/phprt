#!/usr/bin/env bash
# phprt: contract=v2 kind=tool name=perlshim version=PERL_SHIM_VERSION tools=
#
# perl 核心模块补充包（构建期工具用，不是运行时依赖）：
# 从官方 perl 源码包里取出不含 XS 的纯 perl 发行版，放到
# tools/perlshim-<版本>/share/perl5core，需要的地方通过 PERL5LIB 引用。
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds tar xz perl

  local ROOT_D DEST SYSPERL SIG TAR SRC added d
  ROOT_D="$(dep_dir_of perlshim)"
  DEST="$ROOT_D/share/perl5core"
  SYSPERL="$(perl -e 'print $^V' 2>/dev/null || echo unknown)"
  SIG="perlshim=$PERL_SHIM_VERSION;sysperl=$SYSPERL"

  if [ "$FORCE" != 1 ] && stamp_ok perlshim "$SIG" && [ -f "$DEST/IPC/Cmd.pm" ]; then
    ok "perl 核心模块补充包已就绪，跳过"; return 0
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
  cp -a "$SRC/lib/." "$DEST/"

  added=0
  for d in "$SRC"/dist/* "$SRC"/cpan/* "$SRC"/ext/*; do
    [ -d "$d/lib" ] || continue
    find "$d" -name '*.xs' | grep -q . && continue
    cp -a "$d/lib/." "$DEST/"
    added=$((added + 1))
  done

  PERL5LIB="$DEST" perl -MIPC::Cmd -MFindBin -MFile::Compare -MFile::Copy \
    -e 'print "  IPC::Cmd / FindBin / File::Compare / File::Copy 均可加载\n"' \
    || die "perl 模块补充包不完整"

  stamp_write perlshim "$SIG"
  ok "perl 核心模块 -> $DEST（纯 perl 发行版 $added 个 + lib/）"
}

# 上游版本：CPAN src/5.0 目录。Perl 的小版本是奇数表示开发版，只取偶数。
versions() {
  upstream_versions_listing https://www.cpan.org/src/5.0/ \
    'perl-5\.[0-9]+\.[0-9]+\.tar\.xz' |
    awk -F. '$2 % 2 == 0'
}

recipe_main "$@"
