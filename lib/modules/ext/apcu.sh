#!/usr/bin/env bash
# =============================================================================
#  apcu 扩展（PECL 源码 + phpize）—— 进程内共享内存缓存，常驻 worker 里很有用
#  可单独执行: lib/modules/ext/apcu.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

need_cmds gcc make tar

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

AC_DIR="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
SIG="apcu=$APCU_VERSION;$(php_sig)"

if [ "$FORCE" != 1 ] && stamp_ok apcu "$SIG" && [ -f "$PREFIX/etc/conf.d/25-apcu.ini" ]; then
  ok "apcu 已装，跳过"; exit 0
fi

TAR="$(basename "$APCU_URL")"
fetch "$APCU_URL" "$DOWNLOADS/$TAR" "${APCU_SHA256:-}"

SRC="$SRCDIR/apcu-$APCU_VERSION-php-$PHP_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  TMPX="$SRCDIR/.apcu-$APCU_VERSION.$$"
  rm -rf "$TMPX"; mkdir -p "$TMPX"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$TMPX"
  [ -d "$TMPX/apcu-$APCU_VERSION" ] || die "解压后未找到 apcu-$APCU_VERSION"
  mv "$TMPX/apcu-$APCU_VERSION" "$SRC"
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
phpize > "$LOGS/apcu-phpize.log" 2>&1 \
  || { tail -30 "$LOGS/apcu-phpize.log"; die "phpize 失败"; }

log "configure"
./configure --with-php-config="$PREFIX/bin/php-config" --enable-apcu \
  > "$LOGS/apcu-configure.log" 2>&1 \
  || { tail -30 "$LOGS/apcu-configure.log"; die "apcu configure 失败"; }

log "make -j$JOBS"
make -j"$JOBS" > "$LOGS/apcu-make.log" 2>&1 \
  || { tail -40 "$LOGS/apcu-make.log"; die "apcu 编译失败"; }
make install >> "$LOGS/apcu-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/apcu.so" ] || die "没找到 $EXT_DIR/apcu.so"

cat > "$PREFIX/etc/conf.d/25-apcu.ini" <<EOF
; 由 phprt 生成（apcu $APCU_VERSION）
extension=apcu.so
apc.enabled=1
apc.enable_cli=1
apc.shm_size=32M
apc.ttl=0
EOF

stamp_write apcu "$SIG"
ok "apcu $APCU_VERSION -> $EXT_DIR/apcu.so"
