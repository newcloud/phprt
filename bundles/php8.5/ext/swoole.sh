#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=swoole switch=ENABLE_SWOOLE version=SWOOLE_VERSION modules=swoole ini=27-swoole.ini tools=autoconf,perlshim needs=openssl,zlib optional= upstream=pecl:swoole type=shared default=shared category=Messaging
#
# swoole 6.x 的 SSL 代码无条件编译，上游已移除 --enable-openssl；这里只传
# --with-openssl-dir，依赖顺序和头/库路径由 needs= 负责，不会产生
# swoole.so -> libssl 的 DT_NEEDED（运行时由 PHP/curl 加载的 OpenSSL 提供符号）。
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

configure_args() {
  printf '%s\n' \
    --enable-swoole \
    "--with-openssl-dir=$(dep_dir_of openssl)" \
    --enable-sockets \
    --disable-brotli
}

build() {
  need_cmds gcc g++ make tar

  local PHP AC_DIR PERL_DIR SIG TAR SRC LIB_DIRS EXT_DIR a
  PHP="$PREFIX/bin/php"
  [ -x "$PHP" ] || die "还没有 PHP：先编译 PHP 本体"

  AC_DIR="$(dep_dir_of autoconf)"
  PERL_DIR="$(dep_dir_of perlshim)"
  SIG="swoole=$SWOOLE_VERSION;$(php_sig);openssl=${OPENSSL_VERSION:-none};zlib=${ZLIB_VERSION:-none};rpath=1"

  if [ "$FORCE" != 1 ] && stamp_ok swoole "$SIG"; then
    ok "swoole 已编译，跳过"; return 0
  fi

  dep_env swoole

  TAR="$(basename "$SWOOLE_URL")"
  fetch "$SWOOLE_URL" "$DOWNLOADS/$TAR" "${SWOOLE_SHA256:-}"

  SRC="$SRCDIR/swoole-$SWOOLE_VERSION-php-$PHP_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make clean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    local TMPX="$SRCDIR/.swoole-$SWOOLE_VERSION.$$"
    rm -rf "$TMPX"; mkdir -p "$TMPX"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$TMPX"
    [ -d "$TMPX/swoole-$SWOOLE_VERSION" ] || die "解压后未找到 swoole-$SWOOLE_VERSION"
    mv "$TMPX/swoole-$SWOOLE_VERSION" "$SRC"
    rm -rf "$TMPX"
  fi

  # 直接执行 recipe 时也保证工具存在（driver 已经建过就跳过）
  [ -x "$AC_DIR/bin/autoconf" ] || ensure_dep autoconf
  [ -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ] || ensure_dep perlshim

  LIB_DIRS="$(dep_lib_dirs $(enabled_dep_names) | tr '\n' ':' | sed 's/:$//')"
  export PATH="$AC_DIR/bin:$PREFIX/bin:$PATH"
  export LD_LIBRARY_PATH="$PREFIX/lib${LIB_DIRS:+:$LIB_DIRS}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  export PHP_AUTOCONF="$AC_DIR/bin/autoconf" PHP_AUTOHEADER="$AC_DIR/bin/autoheader"
  export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"

  cd "$SRC"
  log "phpize"
  phpize > "$LOGS/swoole-phpize.log" 2>&1 \
    || { tail -30 "$LOGS/swoole-phpize.log"; die "phpize 失败"; }

  log "configure（--enable-swoole --with-openssl-dir）"
  # 不传 --enable-mysqlnd：phprtv2 没有 mysqli/mysqlnd，swoole 的协程 MySQL
  # 客户端因此不可用，其余功能不受影响。
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ./configure --with-php-config="$PREFIX/bin/php-config" "${_args[@]}" \
    > "$LOGS/swoole-configure.log" 2>&1 \
    || { tail -40 "$LOGS/swoole-configure.log"; die "swoole configure 失败"; }

  log "make -j$JOBS（C++，偏慢）"
  make -j"$JOBS" > "$LOGS/swoole-make.log" 2>&1 \
    || { tail -60 "$LOGS/swoole-make.log"; die "swoole 编译失败"; }
  make install >> "$LOGS/swoole-make.log" 2>&1

  EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
  [ -f "$EXT_DIR/swoole.so" ] || die "没找到 $EXT_DIR/swoole.so"

  stamp_write swoole "$SIG"
  ok "swoole $SWOOLE_VERSION -> $EXT_DIR/swoole.so"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/27-swoole.ini" <<EOF
; 由 phprt 生成（swoole $SWOOLE_VERSION）
extension=swoole.so
swoole.use_shortname=Off
EOF
}

recipe_main "$@"
