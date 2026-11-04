#!/usr/bin/env bash
# =============================================================================
#  swoole 扩展（PECL 源码 + phpize）—— 需要 C++ 编译器（g++）
#  phprt: switch=ENABLE_SWOOLE needs=openssl,zlib
#  可单独执行: lib/modules/ext/swoole.sh
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
SIG="swoole=$SWOOLE_VERSION;$(php_sig);rpath=1"

if [ "$FORCE" != 1 ] && stamp_ok swoole "$SIG" && [ -f "$PHPRT_HOME/conf.d/27-swoole.ini" ]; then
  ok "swoole 已装，跳过"; exit 0
fi

# 硬依赖：swoole 无条件 --enable-openssl，并且 configure 里的 AC_CHECK_LIB(z)
# 没有 -L 参数，只能靠这里注入；缺库就先构建（不碰 PHP 的 zlib 扩展开关）
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
log "configure（--enable-swoole --enable-openssl --enable-sockets --enable-mysqlnd）"
./configure --with-php-config="$PREFIX/bin/php-config" \
  --enable-swoole --enable-openssl --with-openssl-dir="$D_OPENSSL" \
  --enable-sockets --enable-mysqlnd --disable-brotli \
  > "$LOGS/swoole-configure.log" 2>&1 \
  || { tail -40 "$LOGS/swoole-configure.log"; die "swoole configure 失败"; }

log "make -j$JOBS（C++，偏慢）"
make -j"$JOBS" > "$LOGS/swoole-make.log" 2>&1 \
  || { tail -60 "$LOGS/swoole-make.log"; die "swoole 编译失败"; }
make install >> "$LOGS/swoole-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/swoole.so" ] || die "没找到 $EXT_DIR/swoole.so"

cat > "$PHPRT_HOME/conf.d/27-swoole.ini" <<EOF
; 由 phprt 生成（swoole $SWOOLE_VERSION）
extension=swoole.so
swoole.use_shortname=Off
EOF

stamp_write swoole "$SIG"
ok "swoole $SWOOLE_VERSION -> $EXT_DIR/swoole.so"
