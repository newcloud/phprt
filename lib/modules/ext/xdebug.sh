#!/usr/bin/env bash
# =============================================================================
#  xdebug 扩展（PECL 源码 + phpize）
#  phprt: switch=ENABLE_XDEBUG needs=zlib
#  默认 xdebug.mode=off，不拖慢运行；调试时 export XDEBUG_MODE=debug 即可
#  可单独执行: lib/modules/ext/xdebug.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

need_cmds gcc make tar

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

AC_DIR="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
SIG="xdebug=$XDEBUG_VERSION;$(php_sig);rpath=1"

if [ "$FORCE" != 1 ] && stamp_ok xdebug "$SIG" && [ -f "$PREFIX/etc/conf.d/30-xdebug.ini" ]; then
  ok "xdebug 已装，跳过"; exit 0
fi

# xdebug 的 profiler 压缩走 pkg-config 找 zlib；不注入的话会拿系统的
ensure_needs xdebug
dep_env xdebug

TAR="$(basename "$XDEBUG_URL")"
fetch "$XDEBUG_URL" "$DOWNLOADS/$TAR" "${XDEBUG_SHA256:-}"

SRC="$SRCDIR/xdebug-$XDEBUG_VERSION-php-$PHP_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  TMPX="$SRCDIR/.xdebug-$XDEBUG_VERSION.$$"
  rm -rf "$TMPX"; mkdir -p "$TMPX"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$TMPX"
  [ -d "$TMPX/xdebug-$XDEBUG_VERSION" ] || die "解压后未找到 xdebug-$XDEBUG_VERSION"
  mv "$TMPX/xdebug-$XDEBUG_VERSION" "$SRC"
  rm -rf "$TMPX"
fi

# phpize 需要 autoconf / perl 核心模块（精简系统可能缺）
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
phpize > "$LOGS/xdebug-phpize.log" 2>&1 \
  || { tail -30 "$LOGS/xdebug-phpize.log"; die "phpize 失败"; }

log "configure"
./configure --with-php-config="$PREFIX/bin/php-config" \
  > "$LOGS/xdebug-configure.log" 2>&1 \
  || { tail -30 "$LOGS/xdebug-configure.log"; die "xdebug configure 失败"; }

log "make -j$JOBS"
make -j"$JOBS" > "$LOGS/xdebug-make.log" 2>&1 \
  || { tail -40 "$LOGS/xdebug-make.log"; die "xdebug 编译失败"; }
make install >> "$LOGS/xdebug-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/xdebug.so" ] || die "没找到 $EXT_DIR/xdebug.so"

cat > "$PREFIX/etc/conf.d/30-xdebug.ini" <<EOF
; 由 phprt 生成（xdebug $XDEBUG_VERSION）
; 默认关掉，不拖慢 RoadRunner worker；调试时 export XDEBUG_MODE=debug
zend_extension=xdebug.so
xdebug.mode=off
xdebug.start_with_request=yes
xdebug.client_host=127.0.0.1
xdebug.client_port=9003
EOF

stamp_write xdebug "$SIG"
ok "xdebug $XDEBUG_VERSION -> $EXT_DIR/xdebug.so"
