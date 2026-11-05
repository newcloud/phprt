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
./config --prefix="$D" --openssldir="$(system_ssldir)" --libdir=lib \
    shared no-tests > "$LOGS/openssl-configure.log" 2>&1 \
  || { tail -20 "$LOGS/openssl-configure.log"; die "OpenSSL Configure 失败"; }
make -j"$JOBS" > "$LOGS/openssl-make.log" 2>&1 \
  || { tail -20 "$LOGS/openssl-make.log"; die "OpenSSL make 失败"; }
# OPENSSLDIR 是系统目录（/etc 下），不能往那里装 openssl.cnf / 证书：
# 这里只装 --prefix 下的库、头文件和命令。CA 由 ensure_tree_ssl 放进
# $TREE/etc/ssl/cert.pem，运行时由 myphp 用 SSL_CERT_FILE 指过去。
# 这样编译产物里没有构建树的绝对路径，整棵树搬走也不影响。
make install_sw >> "$LOGS/openssl-make.log" 2>&1

stamp_write openssl "$SIG"
ok "OpenSSL $OPENSSL_VERSION -> $D"
