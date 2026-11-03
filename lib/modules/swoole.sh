#!/usr/bin/env bash
# =============================================================================
#  swoole 扩展（PECL 源码 + phpize）—— 需要 C++ 编译器（g++）
#  可单独执行: lib/modules/swoole.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
load_config

need_cmds gcc g++ make tar

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

AC_DIR="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
SIG="swoole=$SWOOLE_VERSION;$(php_sig)"

if [ "$FORCE" != 1 ] && stamp_ok swoole "$SIG" && [ -f "$PREFIX/etc/conf.d/27-swoole.ini" ]; then
  ok "swoole 已装，跳过"; exit 0
fi

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
  bash "$MODULES/autoconf.sh"
fi
if [ ! -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ]; then
  bash "$MODULES/perlshim.sh"
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

# openssl / sockets / mysqlnd 用 PHP 自带的那套；不引 c-ares、pgsql 这些额外依赖
log "configure（--enable-swoole --enable-openssl --enable-sockets --enable-mysqlnd）"
./configure --with-php-config="$PREFIX/bin/php-config" \
  --enable-swoole --enable-openssl --enable-sockets --enable-mysqlnd \
  > "$LOGS/swoole-configure.log" 2>&1 \
  || { tail -40 "$LOGS/swoole-configure.log"; die "swoole configure 失败"; }

log "make -j$JOBS（C++，偏慢）"
make -j"$JOBS" > "$LOGS/swoole-make.log" 2>&1 \
  || { tail -60 "$LOGS/swoole-make.log"; die "swoole 编译失败"; }
make install >> "$LOGS/swoole-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/swoole.so" ] || die "没找到 $EXT_DIR/swoole.so"

cat > "$PREFIX/etc/conf.d/27-swoole.ini" <<EOF
; 由 phprt 生成（swoole $SWOOLE_VERSION）
extension=swoole.so
swoole.use_shortname=Off
EOF

stamp_write swoole "$SIG"
ok "swoole $SWOOLE_VERSION -> $EXT_DIR/swoole.so"
