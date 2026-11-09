#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=openssl version=OPENSSL_VERSION needs= optional= tools=perlshim wheel=singleton:libssl+libcrypto upstream=github:openssl/openssl
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc make perl tar
  lto_on deps && need_cmds gcc-ar gcc-ranlib

  local D PERL_DIR SIG TAR SRC
  D="$(dep_dir_of openssl)"
  PERL_DIR="$(dep_dir_of perlshim)"
  SIG="openssl=$OPENSSL_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok openssl "$SIG" && [ -f "$D/lib/libssl.so" ]; then
    ok "OpenSSL 已安装，跳过"; return 0
  fi

  ensure_clean_target openssl "$D"

  TAR="$(basename "$OPENSSL_URL")"
  fetch "$OPENSSL_URL" "$DOWNLOADS/$TAR" "${OPENSSL_SHA256:-}"

  SRC="$SRCDIR/openssl-$OPENSSL_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
    [ -d "$SRC" ] || die "解压后未找到 $SRC"
  fi

  # 精简系统缺 perl-core（FindBin / IPC::Cmd…），先用 perl 源码包补齐纯 perl 模块
  if [ ! -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ]; then
    log "补齐 perl 核心模块（构建期需要）"
    ensure_dep perlshim
  fi
  export PERL5LIB="$PERL_DIR/share/perl5core:$VENDOR/perl${PERL5LIB:+:$PERL5LIB}"
  perl -MTime::Piece -e1 >/dev/null 2>&1 || export PERL5LIB="$PERL5LIB:$VENDOR/perl-optional"

  cd "$SRC"
  if lto_on deps; then
    # OpenSSL 的 target 默认是 -Wall -O3；用户 CFLAGS 会覆盖 target，
    # 所以这里显式带上基线，避免 LTO 构建反而降优化。
    export CFLAGS="${CFLAGS:--Wall -O3} $LTO_CFLAGS"
    export LDFLAGS="${LDFLAGS:+$LDFLAGS }$LTO_LDFLAGS"
    export AR=gcc-ar RANLIB=gcc-ranlib
  fi
  log "Configure + make -j$JOBS"
  ./config --prefix="$D" --openssldir="$(system_ssldir)" --libdir=lib \
      shared no-tests > "$LOGS/openssl-configure.log" 2>&1 \
    || { tail -20 "$LOGS/openssl-configure.log"; die "OpenSSL Configure 失败"; }
  make -j"$JOBS" AR="${AR:-ar}" RANLIB="${RANLIB:-ranlib}" \
      > "$LOGS/openssl-make.log" 2>&1 \
    || { tail -20 "$LOGS/openssl-make.log"; die "OpenSSL make 失败"; }
  # OPENSSLDIR 指向系统目录，不能往那里装配置和证书；只装 --prefix 下的东西
  make install_sw AR="${AR:-ar}" RANLIB="${RANLIB:-ranlib}" \
      >> "$LOGS/openssl-make.log" 2>&1

  stamp_write openssl "$SIG"
  ok "OpenSSL $OPENSSL_VERSION -> $D"
}

recipe_main "$@"
