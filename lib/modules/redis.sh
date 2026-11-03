#!/usr/bin/env bash
# =============================================================================
#  redis 扩展（PECL 源码 + phpize 编译，产物是 PHP 目录里的 extensions/.../redis.so）
#  源码目录按 PHP 版本分开: build/redis-<版本>-php-<PHP 版本>/
#  可单独执行: lib/modules/redis.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
load_config

need_cmds gcc make tar

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

AC_DIR="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
SIG="redis=$REDIS_VERSION;$(php_sig)"

if [ "$FORCE" != 1 ] && stamp_ok redis "$SIG" && [ -f "$PREFIX/etc/conf.d/20-redis.ini" ]; then
  ok "redis 扩展已装，跳过"; exit 0
fi

TAR="$(basename "$REDIS_URL")"
fetch "$REDIS_URL" "$DOWNLOADS/$TAR" "${REDIS_SHA256:-}"

SRC="$SRCDIR/redis-$REDIS_VERSION-php-$PHP_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  TMPX="$SRCDIR/.redis-$REDIS_VERSION.$$"
  rm -rf "$TMPX"; mkdir -p "$TMPX"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$TMPX"
  [ -d "$TMPX/redis-$REDIS_VERSION" ] || die "解压后未找到 redis-$REDIS_VERSION"
  # phpize 有状态，源码目录按 PHP 版本分开，避免两个版本互相踩
  mv "$TMPX/redis-$REDIS_VERSION" "$SRC"
  rm -rf "$TMPX"
fi

# phpize 需要 autoconf（没有就装到 tools/）
if [ ! -x "$AC_DIR/bin/autoconf" ]; then
  log "编译 redis 需要 autoconf，先安装"
  bash "$MODULES/autoconf.sh"
fi
# 精简系统上 phpize/configure 会缺 perl 核心模块
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
phpize > "$LOGS/redis-phpize.log" 2>&1 \
  || { tail -30 "$LOGS/redis-phpize.log"; die "phpize 失败"; }

log "configure（可选 serializer 全部用 redis 自带实现，不引入 igbinary/msgpack/zstd）"
./configure --with-php-config="$PREFIX/bin/php-config" \
  --enable-redis --enable-redis-session --enable-redis-json \
  > "$LOGS/redis-configure.log" 2>&1 \
  || { tail -30 "$LOGS/redis-configure.log"; die "redis configure 失败"; }

log "make -j$JOBS"
make -j"$JOBS" > "$LOGS/redis-make.log" 2>&1 \
  || { tail -40 "$LOGS/redis-make.log"; die "redis 编译失败"; }
make install >> "$LOGS/redis-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/redis.so" ] || die "没找到 $EXT_DIR/redis.so"

mkdir -p "$PREFIX/etc/conf.d"
# 只写文件名：extension_dir 由 php.ini / myphp 给出，整个安装根搬走也有效
cat > "$PREFIX/etc/conf.d/20-redis.ini" <<EOF
; 由 phprt 生成（redis $REDIS_VERSION）
extension=redis.so
EOF

stamp_write redis "$SIG"
ok "redis $REDIS_VERSION -> $EXT_DIR/redis.so"
