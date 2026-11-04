#!/usr/bin/env bash
# =============================================================================
#  OpenSSL（PHP openssl 扩展、libcurl TLS、Composer 必需）
#  安装到: deps/openssl-<版本>/
#  可单独执行: lib/modules/deps/openssl.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make perl tar

D="$(dep_dir_of openssl)"
PERL_DIR="$(dep_dir_of perl5core)"
SIG="openssl=$OPENSSL_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok openssl "$SIG" && [ -f "$D/lib/libssl.so" ]; then
  ok "OpenSSL 已安装，跳过"; exit 0
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
  bash "$(module_sh perlshim)"
fi
export PERL5LIB="$PERL_DIR/share/perl5core:$VENDOR/perl${PERL5LIB:+:$PERL5LIB}"
# Time::Piece 是 XS 模块，系统没有、又没有 perl 头文件时编不出来，用自带的纯 perl 版
perl -MTime::Piece -e1 >/dev/null 2>&1 || export PERL5LIB="$PERL5LIB:$VENDOR/perl-optional"

cd "$SRC"
log "Configure + make -j$JOBS（约 1-2 分钟）"
./config --prefix="$D" --openssldir="$D/etc/ssl" --libdir=lib \
    shared no-tests > "$LOGS/openssl-configure.log" 2>&1 \
  || { tail -20 "$LOGS/openssl-configure.log"; die "OpenSSL Configure 失败"; }
make -j"$JOBS" > "$LOGS/openssl-make.log" 2>&1 \
  || { tail -20 "$LOGS/openssl-make.log"; die "OpenSSL make 失败"; }
make install_sw >> "$LOGS/openssl-make.log" 2>&1
# install_sw 不装 openssl.cnf，但 PHP 的 openssl 扩展会去 OPENSSLDIR 找它（找不到报
# "configuration file routines::no such file"），所以把 ssl 目录也装出来
make install_ssldirs >> "$LOGS/openssl-make.log" 2>&1 || warn "install_ssldirs 失败（openssl.cnf 缺失）"
[ -f "$D/etc/ssl/openssl.cnf" ] || warn "没找到 $D/etc/ssl/openssl.cnf"

stamp_write openssl "$SIG"
ok "OpenSSL $OPENSSL_VERSION -> $D"
