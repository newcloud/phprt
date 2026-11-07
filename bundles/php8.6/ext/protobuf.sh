#!/usr/bin/env bash
# =============================================================================
#  phprt: switch=ENABLE_PROTOBUF
#  protobuf 扩展（PECL 源码 + phpize）—— Google Protocol Buffers 的 C 运行时
#  纯 C，自带精简版 upb，不依赖外部 libprotobuf，也不需要重编 PHP
#  可单独执行: lib/modules/ext/protobuf.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体（./build.sh --only php）"

AC_DIR="$(dep_dir_of autoconf)"
PERL_DIR="$(dep_dir_of perl5core)"
SIG="protobuf=$PROTOBUF_VERSION;$(php_sig)"

INI="$PREFIX/etc/conf.d/23-protobuf.ini"
if [ "$FORCE" != 1 ] && stamp_ok protobuf "$SIG" && [ -f "$INI" ]; then
  ok "protobuf 已装，跳过"; exit 0
fi

TAR="$(basename "$PROTOBUF_URL")"
fetch "$PROTOBUF_URL" "$DOWNLOADS/$TAR" "${PROTOBUF_SHA256:-}"

SRC="$SRCDIR/protobuf-$PROTOBUF_VERSION-php-$PHP_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  TMPX="$SRCDIR/.protobuf-$PROTOBUF_VERSION.$$"
  rm -rf "$TMPX"; mkdir -p "$TMPX"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$TMPX"
  [ -d "$TMPX/protobuf-$PROTOBUF_VERSION" ] || die "解压后未找到 protobuf-$PROTOBUF_VERSION"
  mv "$TMPX/protobuf-$PROTOBUF_VERSION" "$SRC"
  rm -rf "$TMPX"
fi

if [ ! -x "$AC_DIR/bin/autoconf" ]; then
  log "编译 PECL 扩展需要 autoconf，先安装"
  bash "$(module_sh autoconf)"
fi

# PHP 8.6 移除了 zval_dtor()（8.3 起弃用），等价替换为 zval_ptr_dtor()；
# 5.36.2 和 5.37.0RC1 都还没改全，先在源码上打掉（幂等）
if grep -rqs 'zval_dtor(' "$SRC" --include='*.c' --include='*.h'; then
  log "兼容补丁：zval_dtor -> zval_ptr_dtor（PHP 8.6 已移除）"
  grep -rls 'zval_dtor(' "$SRC" --include='*.c' --include='*.h' \
    | while IFS= read -r f; do sed -i 's/\bzval_dtor(/zval_ptr_dtor(/g' "$f"; done
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
phpize > "$LOGS/protobuf-phpize.log" 2>&1 \
  || { tail -30 "$LOGS/protobuf-phpize.log"; die "phpize 失败"; }

log "configure"
./configure --with-php-config="$PREFIX/bin/php-config" --enable-protobuf \
  > "$LOGS/protobuf-configure.log" 2>&1 \
  || { tail -30 "$LOGS/protobuf-configure.log"; die "protobuf configure 失败"; }

log "make -j$JOBS"
make -j"$JOBS" > "$LOGS/protobuf-make.log" 2>&1 \
  || { tail -40 "$LOGS/protobuf-make.log"; die "protobuf 编译失败"; }
make install >> "$LOGS/protobuf-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/protobuf.so" ] || die "没找到 $EXT_DIR/protobuf.so"

mkdir -p "$PREFIX/etc/conf.d"
cat > "$INI" <<EOF
; 由 phprt 生成（protobuf $PROTOBUF_VERSION）
extension=protobuf.so
EOF

stamp_write protobuf "$SIG"
ok "protobuf $PROTOBUF_VERSION -> $EXT_DIR/protobuf.so"
