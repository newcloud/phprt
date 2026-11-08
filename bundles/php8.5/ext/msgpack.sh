#!/usr/bin/env bash
# =============================================================================
#  phprt: switch=ENABLE_MSGPACK
#  msgpack 扩展（PECL 源码 + phpize 编译，产物是 PHP 目录里的 extensions/.../msgpack.so）
#  源码目录按 PHP 版本分开: build/msgpack-<版本>-php-<PHP 版本>/
#  可单独执行: lib/modules/ext/msgpack.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

AC_DIR="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
SIG="msgpack=$MSGPACK_VERSION;$(php_sig)"

if [ "$FORCE" != 1 ] && stamp_ok msgpack "$SIG" && [ -f "$PREFIX/etc/conf.d/20-msgpack.ini" ]; then
  ok "msgpack 扩展已装，跳过"; exit 0
fi

TAR="$(basename "$MSGPACK_URL")"
fetch "$MSGPACK_URL" "$DOWNLOADS/$TAR" "${MSGPACK_SHA256:-}"

SRC="$SRCDIR/msgpack-$MSGPACK_VERSION-php-$PHP_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  TMPX="$SRCDIR/.msgpack-$MSGPACK_VERSION.$$"
  rm -rf "$TMPX"; mkdir -p "$TMPX"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$TMPX"
  [ -d "$TMPX/msgpack-$MSGPACK_VERSION" ] || die "解压后未找到 msgpack-$MSGPACK_VERSION"
  # phpize 有状态，源码目录按 PHP 版本分开，避免两个版本互相踩
  mv "$TMPX/msgpack-$MSGPACK_VERSION" "$SRC"
  rm -rf "$TMPX"
fi

# phpize 需要 autoconf（没有就装到 tools/）
if [ ! -x "$AC_DIR/bin/autoconf" ]; then
  log "编译 msgpack 需要 autoconf，先安装"
  bash "$(module_sh autoconf)"
fi
# 精简系统上 phpize/configure 会缺 perl 核心模块
if [ ! -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ]; then
  bash "$(module_sh perlshim)"
fi

export PATH="$AC_DIR/bin:$PREFIX/bin:$PATH"
export PHP_AUTOCONF="$AC_DIR/bin/autoconf" PHP_AUTOHEADER="$AC_DIR/bin/autoheader"
export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"

cd "$SRC"

log "phpize"
phpize > "$LOGS/msgpack-phpize.log" 2>&1 \
  || { tail -30 "$LOGS/msgpack-phpize.log"; die "phpize 失败"; }

log "configure（--with-msgpack；源码自带 bundled msgpack-c，无外部库依赖）"
./configure --with-php-config="$PREFIX/bin/php-config" \
  --with-msgpack \
  > "$LOGS/msgpack-configure.log" 2>&1 \
  || { tail -30 "$LOGS/msgpack-configure.log"; die "msgpack configure 失败"; }

log "make -j$JOBS"
make -j"$JOBS" > "$LOGS/msgpack-make.log" 2>&1 \
  || { tail -40 "$LOGS/msgpack-make.log"; die "msgpack 编译失败"; }
make install >> "$LOGS/msgpack-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/msgpack.so" ] || die "没找到 $EXT_DIR/msgpack.so"

mkdir -p "$PREFIX/etc/conf.d"
# 只写文件名：extension_dir 由 php.ini / myphp 给出，整个安装根搬走也有效
cat > "$PREFIX/etc/conf.d/20-msgpack.ini" <<EOF
; 由 phprt 生成（msgpack $MSGPACK_VERSION）
extension=msgpack.so
EOF

stamp_write msgpack "$SIG"
ok "msgpack $MSGPACK_VERSION -> $EXT_DIR/msgpack.so"
