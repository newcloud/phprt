#!/usr/bin/env bash
# =============================================================================
#  phprt: switch=ENABLE_IGBINARY
#  igbinary 扩展（PECL 源码 + phpize 编译，产物是 PHP 目录里的 extensions/.../igbinary.so）
#  源码目录按 PHP 版本分开: build/igbinary-<版本>-php-<PHP 版本>/
#  可单独执行: lib/modules/ext/igbinary.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

AC_DIR="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
SIG="igbinary=$IGBINARY_VERSION;$(php_sig)"

if [ "$FORCE" != 1 ] && stamp_ok igbinary "$SIG" && [ -f "$PREFIX/etc/conf.d/20-igbinary.ini" ]; then
  ok "igbinary 扩展已装，跳过"; exit 0
fi

TAR="$(basename "$IGBINARY_URL")"
fetch "$IGBINARY_URL" "$DOWNLOADS/$TAR" "${IGBINARY_SHA256:-}"

SRC="$SRCDIR/igbinary-$IGBINARY_VERSION-php-$PHP_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  TMPX="$SRCDIR/.igbinary-$IGBINARY_VERSION.$$"
  rm -rf "$TMPX"; mkdir -p "$TMPX"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$TMPX"
  [ -d "$TMPX/igbinary-$IGBINARY_VERSION" ] || die "解压后未找到 igbinary-$IGBINARY_VERSION"
  # phpize 有状态，源码目录按 PHP 版本分开，避免两个版本互相踩
  mv "$TMPX/igbinary-$IGBINARY_VERSION" "$SRC"
  rm -rf "$TMPX"
fi

# phpize 需要 autoconf（没有就装到 tools/）
if [ ! -x "$AC_DIR/bin/autoconf" ]; then
  log "编译 igbinary 需要 autoconf，先安装"
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
phpize > "$LOGS/igbinary-phpize.log" 2>&1 \
  || { tail -30 "$LOGS/igbinary-phpize.log"; die "phpize 失败"; }

# PHP 8.4 起 ext/standard/php_smart_string.h 被移除；igbinary 3.2.16 只 include
# 不用它（全源码无任何 smart_str/smart_string 调用），删掉即可编译
if ! grep -q 'PHPRT_PHP84_COMPAT' src/php7/php_igbinary.h; then
  log "PHP 8.4+ 兼容补丁（删除未使用的 php_smart_string.h include）"
  sed -i '\|#include "ext/standard/php_smart_string.h"|d' src/php7/php_igbinary.h
  echo '/* PHPRT_PHP84_COMPAT: php_smart_string.h removed in PHP 8.4; include was unused */' \
    >> src/php7/php_igbinary.h
fi

log "configure（--enable-igbinary；无外部库依赖）"
./configure --with-php-config="$PREFIX/bin/php-config" \
  --enable-igbinary \
  > "$LOGS/igbinary-configure.log" 2>&1 \
  || { tail -30 "$LOGS/igbinary-configure.log"; die "igbinary configure 失败"; }

log "make -j$JOBS"
make -j"$JOBS" > "$LOGS/igbinary-make.log" 2>&1 \
  || { tail -40 "$LOGS/igbinary-make.log"; die "igbinary 编译失败"; }
make install >> "$LOGS/igbinary-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/igbinary.so" ] || die "没找到 $EXT_DIR/igbinary.so"

mkdir -p "$PREFIX/etc/conf.d"
# 只写文件名：extension_dir 由 php.ini / myphp 给出，整个安装根搬走也有效
cat > "$PREFIX/etc/conf.d/20-igbinary.ini" <<EOF
; 由 phprt 生成（igbinary $IGBINARY_VERSION）
extension=igbinary.so
EOF

stamp_write igbinary "$SIG"
ok "igbinary $IGBINARY_VERSION -> $EXT_DIR/igbinary.so"
