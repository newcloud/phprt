#!/usr/bin/env bash
# PHP 本体：只负责根据最终 configure 参数编译 PHP。
# 依赖由 build.sh 的 deps 阶段先构建，这里不再隐式建依赖。
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config
need_cmds gcc make tar

BUILTIN_ORDER="openssl zlib libxml2 curl sqlite mysqli mbstring intl zip gd ffi"

build_args() {
  local e f sw v
  for e in $BUILTIN_ORDER; do
    f="$BUNDLE/builtin/$e.sh"
    [ -f "$f" ] || continue
    sw="$(sed -n 's/^# *phprt:.*[[:space:]]switch=\([^[:space:]]*\).*/\1/p' "$f" | head -1)"
    [ -n "$sw" ] || continue
    if printf ' %s ' "$SELECTED_EXTS" | grep -q " $e "; then v=1; else v=0; fi
    printf -v "$sw" '%s' "$v"
    export "${sw?}"
  done

  ARGS=(
    --prefix="$TREE"
    --with-config-file-path="$TREE/etc"
    --with-config-file-scan-dir="$TREE/etc/conf.d"
    --enable-cli --disable-cgi
    --without-pear
    # OPcache 自 PHP 8.5 起固定编入 PHP，--enable-opcache 已被上游移除
    --enable-filter --enable-phar --enable-fileinfo
    --enable-bcmath --enable-calendar --enable-exif
    --enable-pcntl --enable-posix --enable-sockets
    --enable-shmop --enable-sysvmsg --enable-sysvsem --enable-sysvshm
  )
  for e in $BUILTIN_ORDER; do
    f="$BUNDLE/builtin/$e.sh"
    [ -f "$f" ] || continue
    while IFS= read -r a; do ARGS+=("$a"); done < <(bash "$f" --flags)
  done
}

build_args

# 只打印最终 configure 参数，供 build.sh 计算 PHP 签名
if [ "${1:-}" = "--flags" ]; then
  printf '%s\n' "${ARGS[@]}"
  exit 0
fi

ensure_source "$PHP_ARCHIVE" "$PHP_URL" "$PHP_SHA256"
SRC="$SRCDIR/$PHP_SRC_DIR"
if [ ! -d "$SRC" ]; then
  rm -rf "$SRC"
  extract "$PHPRT_HOME/downloads/$PHP_ARCHIVE" "$SRCDIR"
else
  log "复用源码 $SRC"
fi
[ -d "$SRC" ] || die "解压后找不到 $SRC"
cd "$SRC"

if [ "${NEEDS_BUILDCONF:-0}" = 1 ] && [ ! -x "$SRC/configure" ]; then
  AC_DIR="$(dep_dir_of autoconf)"
  RE2C_DIR="$(dep_dir_of re2c)"
  PERL_DIR="$(dep_dir_of perl5core)"
  [ -x "$AC_DIR/bin/autoconf" ] || bash "$(module_sh autoconf)"
  [ -x "$RE2C_DIR/bin/re2c" ] || bash "$(module_sh re2c)"
  [ -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ] || bash "$(module_sh perlshim)"
  export PATH="$AC_DIR/bin:$RE2C_DIR/bin:$PATH"
  export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"
  log "buildconf"
  ./buildconf --force > "$LOGS/php-buildconf.log" 2>&1 ||
    { tail -30 "$LOGS/php-buildconf.log"; die "buildconf 失败（需要 autoconf/re2c）"; }
fi

names="$(enabled_dep_names)"
export CPPFLAGS="$(dep_include_flags $names)${CPPFLAGS:+ $CPPFLAGS}"
export LDFLAGS="$(dep_build_flags $names)${LDFLAGS:+ $LDFLAGS}"
export PKG_CONFIG_PATH="$(dep_pc_path $names)${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

log "PHP configure"
./configure "${ARGS[@]}" > "$LOGS/php-configure.log" 2>&1 ||
  { tail -60 "$LOGS/php-configure.log"; die "PHP configure 失败"; }
{
  printf 'configure: ./configure'
  printf ' %q' "${ARGS[@]}"
  printf '\n'
} > "$TREE/.configure-line"

log "PHP make -j$JOBS"
make -j"$JOBS" > "$LOGS/php-make.log" 2>&1 ||
  { tail -60 "$LOGS/php-make.log"; die "PHP make 失败"; }
log "PHP make install"
make install > "$LOGS/php-install.log" 2>&1 ||
  { tail -40 "$LOGS/php-install.log"; die "PHP make install 失败"; }

[ -x "$TREE/bin/php" ] || die "安装后没有 $TREE/bin/php"
[ -f "$TREE/bin/phar.phar" ] || die "安装后没有 phar.phar（phar 命令不可用）"
ok "PHP $PHP_VERSION 本体完成"
