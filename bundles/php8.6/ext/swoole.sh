#!/usr/bin/env bash
# =============================================================================
#  swoole 扩展（PECL 源码 + phpize）—— 需要 C++ 编译器（g++）
#  phprt: switch=ENABLE_SWOOLE needs=openssl,zlib
#  可单独执行: lib/modules/ext/swoole.sh
# =============================================================================
# swoole 与 OpenSSL 的依赖关系（这段说明不要删除）
#
# swoole 依赖 OpenSSL。swoole 6.x 的 SSL 代码是无条件编译的，上游已移除
# --enable-openssl；本脚本不再传该参数（传了只会得到 configure 警告）。phprt 的
# `needs=openssl` 只负责构建顺序和头/库路径，不会写成 DT_NEEDED。
#
# `ldd swoole.so` / `readelf -d swoole.so` 看不到 libssl.so 是预期现象：
# swoole.so 没有 libssl 的 DT_NEEDED，SSL_* 符号在运行时由 PHP 直接或
# 间接加载的 libssl 提供：
#   bin/php → NEEDED libssl/libcrypto（PHP 内置 openssl）
#   bin/php → NEEDED libcurl → libssl（curl 的 openssl）
# 两个来源都没有时，swoole 第一次 TLS 操作会报 undefined symbol。
#
# 这是否是 swoole 有意避免成为 OpenSSL 的独立链接方，以降低 PHP 扩展
# 生态里多版本共存的风险？从提交历史看不是：dea57f44cc（PR #5965）
# 把 OpenSSL 改成必需时删掉了 PHP_ADD_LIBRARY(ssl/crypto, 1,
# SWOOLE_SHARED_LIBADD)，但同一个提交保留的 pkg-config 分支仍然链接
# libssl/libcrypto。两个分支行为不一致，更像重构遗漏，而不是有意设计。
#
# 即使 swoole 显式 NEEDED libssl，也不会产生第二份 OpenSSL：同一个
# SONAME/文件会被加载器复用。真正会多版本共存的是静态链接 OpenSSL、
# dlopen 另一份不同 SONAME 的库，或给 swoole 单独打包一份 libssl。
# 因此必须保持：一棵 PHP 树 / 一个进程只允许一份 OpenSSL，主版本一致
# （PHP、curl、libzip、swoole 全部使用同一个 3.5.x）。
#
# 详见 docs/SWOOLE-OPENSSL.md。
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc g++ make tar

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

AC_DIR="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
D_OPENSSL="$(dep_dir_of openssl)"
SIG="swoole=$SWOOLE_VERSION;$(php_sig);openssl=${OPENSSL_VERSION:-none};zlib=${ZLIB_VERSION:-none};rpath=1"

if [ "$FORCE" != 1 ] && stamp_ok swoole "$SIG" && [ -f "$PREFIX/etc/conf.d/27-swoole.ini" ]; then
  ok "swoole 已装，跳过"; exit 0
fi

# 硬依赖：swoole 依赖 OpenSSL；configure 里的 AC_CHECK_LIB(z) 没有 -L 参数，
# 只能靠这里注入；缺库就先构建（不碰 PHP 的 zlib 扩展开关）。
ensure_needs swoole
dep_env swoole

TAR="$(basename "$SWOOLE_URL")"
fetch "$SWOOLE_URL" "$DOWNLOADS/$TAR" "${SWOOLE_SHA256:-}"

SRC="$SRCDIR/swoole-$SWOOLE_VERSION-php-$PHP_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  TMPX="$SRCDIR/.swoole-$SWOOLE_VERSION.$$"
  rm -rf "$TMPX"; mkdir -p "$TMPX"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$TMPX"
  [ -d "$TMPX/swoole-$SWOOLE_VERSION" ] || die "解压后未找到 swoole-$SWOOLE_VERSION"
  mv "$TMPX/swoole-$SWOOLE_VERSION" "$SRC"
  rm -rf "$TMPX"
fi

if [ ! -x "$AC_DIR/bin/autoconf" ]; then
  log "编译 PECL 扩展需要 autoconf，先安装"
  bash "$(module_sh autoconf)"
fi
if [ ! -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ]; then
  bash "$(module_sh perlshim)"
fi

LIB_DIRS="$(dep_lib_dirs $(enabled_dep_names) | tr '\n' ':' | sed 's/:$//')"
export PATH="$AC_DIR/bin:$PREFIX/bin:$PATH"
export LD_LIBRARY_PATH="$PREFIX/lib${LIB_DIRS:+:$LIB_DIRS}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export PHP_AUTOCONF="$AC_DIR/bin/autoconf" PHP_AUTOHEADER="$AC_DIR/bin/autoheader"
export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"

cd "$SRC"
log "phpize"
phpize > "$LOGS/swoole-phpize.log" 2>&1 \
  || { tail -30 "$LOGS/swoole-phpize.log"; die "phpize 失败"; }

# 用我们自己编的 OpenSSL；brotli 用不到就关掉（否则会链系统 libbrotli*）
log "configure（--enable-swoole --with-openssl-dir --enable-sockets --enable-mysqlnd）"
./configure --with-php-config="$PREFIX/bin/php-config" \
  --enable-swoole --with-openssl-dir="$D_OPENSSL" \
  --enable-sockets --enable-mysqlnd --disable-brotli \
  > "$LOGS/swoole-configure.log" 2>&1 \
  || { tail -40 "$LOGS/swoole-configure.log"; die "swoole configure 失败"; }

log "make -j$JOBS（C++，偏慢）"
make -j"$JOBS" > "$LOGS/swoole-make.log" 2>&1 \
  || { tail -60 "$LOGS/swoole-make.log"; die "swoole 编译失败"; }
make install >> "$LOGS/swoole-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/swoole.so" ] || die "没找到 $EXT_DIR/swoole.so"

mkdir -p "$PREFIX/etc/conf.d"
cat > "$PREFIX/etc/conf.d/27-swoole.ini" <<EOF
; 由 phprt 生成（swoole $SWOOLE_VERSION）
extension=swoole.so
swoole.use_shortname=Off
EOF

stamp_write swoole "$SIG"
ok "swoole $SWOOLE_VERSION -> $EXT_DIR/swoole.so"
