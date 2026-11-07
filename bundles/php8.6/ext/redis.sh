#!/usr/bin/env bash
# =============================================================================
#  phprt: switch=ENABLE_REDIS
#  redis 扩展（PECL 源码 + phpize 编译，产物是 PHP 目录里的 extensions/.../redis.so）
#  源码目录按 PHP 版本分开: build/redis-<版本>-php-<PHP 版本>/
#  可单独执行: lib/modules/ext/redis.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
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
  bash "$(module_sh autoconf)"
fi
# 精简系统上 phpize/configure 会缺 perl 核心模块
if [ ! -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ]; then
  bash "$(module_sh perlshim)"
fi

LIB_DIRS="$(dep_lib_dirs $(enabled_dep_names) | tr '\n' ':' | sed 's/:$//')"
export PATH="$AC_DIR/bin:$PREFIX/bin:$PATH"
export LD_LIBRARY_PATH="$PREFIX/lib${LIB_DIRS:+:$LIB_DIRS}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export PHP_AUTOCONF="$AC_DIR/bin/autoconf" PHP_AUTOHEADER="$AC_DIR/bin/autoheader"
export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"

cd "$SRC"

# PHP 8.6 移除了 INI_STR/INI_INT/INI_BOOL/XtOffsetOf/zval_dtor/zval_is_true
if [ "${PHP_VERSION#8.6}" != "$PHP_VERSION" ]; then
  log "PHP 8.6 兼容补丁"
  if ! grep -q 'PHPREDIS_PHP86_COMPAT' common.h; then
    cat >> common.h <<'EOF'

/* PHPREDIS_PHP86_COMPAT */
#if PHP_VERSION_ID >= 80600
# ifndef INI_STR
#  define INI_STR(name) zend_ini_string_ex((name), strlen(name), 0, NULL)
# endif
# ifndef INI_INT
#  define INI_INT(name) zend_ini_long((name), strlen(name), 0)
# endif
# ifndef INI_BOOL
#  define INI_BOOL(name) ((bool) INI_INT(name))
# endif
# ifndef XtOffsetOf
#  define XtOffsetOf(s_type, field) offsetof(s_type, field)
# endif
#endif
EOF
  fi
  grep -rls 'zval_dtor(' . --include='*.c' --include='*.h' \
    | while IFS= read -r f; do sed -i 's/\bzval_dtor(/zval_ptr_dtor(/g' "$f"; done
  grep -rls 'zval_is_true(' . --include='*.c' --include='*.h' \
    | while IFS= read -r f; do sed -i 's/\bzval_is_true(/zend_is_true(/g' "$f"; done
fi

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
