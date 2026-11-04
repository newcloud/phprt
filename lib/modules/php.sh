#!/usr/bin/env bash
# =============================================================================
#  PHP 本体
#  安装到: php-<版本>/（每个 PHP 版本一套，互不干扰）
#  可单独执行: lib/modules/php.sh
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
load_config

need_cmds gcc make bison tar xz

# ------------------------------------------------------------- 参数 ------
# 内置扩展的开关 / configure 参数 / 依赖都在 lib/modules/builtin/<扩展>.sh，
# 这里只按固定顺序收集，外加不依赖外部库的公共扩展和依赖库 rpath。
php_build_args() {
  ARGS=(
    --prefix="$PREFIX"
    --with-config-file-path="$PREFIX/etc"
    --with-config-file-scan-dir="$PREFIX/etc/conf.d"
    --enable-cli --disable-cgi
    --without-pear
    --enable-phar --enable-filter --enable-fileinfo
    --enable-opcache
  )

  local e f a
  for e in $(builtin_order); do
    f="$(builtin_sh "$e")"
    [ -n "$f" ] || continue
    while IFS= read -r a; do ARGS+=("$a"); done < <(bash "$f" --flags)
  done

  # 常用内置扩展（不依赖外部库，全开）
  ARGS+=(
    --enable-bcmath --enable-calendar --enable-exif
    --enable-pcntl --enable-posix --enable-sockets
    --enable-shmop --enable-sysvmsg --enable-sysvsem --enable-sysvshm
  )

  # 依赖库全在 deps/<名字>-<版本>/，链接时把每个 lib 都写进 rpath；
  # myphp 运行时还会设 LD_LIBRARY_PATH，所以整个安装根搬走也能用。
  mapfile -t LIB_DIRS < <(dep_lib_dirs $(enabled_dep_names))
  LDFLAGS=""
  CPPFLAGS=""
  local d rpath="" pc=""
  for d in ${LIB_DIRS[@]+"${LIB_DIRS[@]}"}; do
    LDFLAGS+="-L$d "
    rpath+="$d:"
    CPPFLAGS+="-I$(dirname "$d")/include "
    [ -d "$d/pkgconfig" ] && pc+="$d/pkgconfig:"
  done
  rpath="${rpath%:}"
  export PKG_CONFIG_PATH="${pc%:}${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  [ -n "$rpath" ] && LDFLAGS+="-Wl,-rpath,$rpath -Wl,--enable-new-dtags"
  return 0
}

php_build_args
SIG="$(php_sig)"

# ./build.sh --show-args：只打印最终 configure 行，不编译
if [ "${PHPRT_SHOW_ARGS:-0}" = 1 ]; then
  printf 'CPPFLAGS=%q LDFLAGS=%q\n' "$CPPFLAGS" "$LDFLAGS"
  printf './configure'
  printf ' %q' "${ARGS[@]}"
  printf '\n'
  exit 0
fi

if [ "$FORCE" != 1 ] && [ "$REBUILD" != 1 ] && php_is_current; then
  ok "PHP 已安装且编译配置未变（--rebuild 可重编）"
  exit 0
fi

ensure_clean_target php "$PREFIX"

TAR="$(basename "$PHP_URL")"
fetch "$PHP_URL" "$DOWNLOADS/$TAR" "${PHP_SHA256:-}"

# 正式包解压出来是 php-<版本>/；GitHub 分支包是 php-src-<分支>/，用 PHP_SRC_DIR 指定
SRC="$SRCDIR/${PHP_SRC_DIR:-php-$PHP_VERSION}"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码目录 $SRC"
  ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  log "解压 $TAR -> build/"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  if [ ! -d "$SRC" ]; then
    # 兜底：src/ 下唯一一个带 configure 的 php* 目录
    SRC="$(find "$SRCDIR" -maxdepth 2 -type f -name configure -path '*php*' -printf '%h\n' 2>/dev/null | head -1)"
    [ -n "$SRC" ] && [ -d "$SRC" ] || die "解压后没找到 PHP 源码目录（src/ 里没有 configure）"
  fi
fi

# ------------------------------------------------------------------ 编译 ----
cd "$SRC"

# GitHub 的分支/tag 归档不带 configure（官方 release 包才带），先 buildconf 生成
if [ ! -x "$SRC/configure" ]; then
  AC_DIR="$(dep_dir_of autoconf)"
  RE2C_DIR="$(dep_dir_of re2c)"
  PERL_DIR="$(dep_dir_of perl5core)"
  [ -x "$AC_DIR/bin/autoconf" ] || bash "$(module_sh autoconf)"
  [ -x "$RE2C_DIR/bin/re2c" ]   || bash "$(module_sh re2c)"
  [ -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ] || bash "$(module_sh perlshim)"
  # 注意要 export：后面的 configure / make 也要能找到 re2c
  export PATH="$AC_DIR/bin:$RE2C_DIR/bin:$PREFIX/bin:$PATH"
  export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"
  log "源码包没有 configure，跑 buildconf 生成"
  ./buildconf --force > "$LOGS/php-buildconf.log" 2>&1 \
    || { tail -30 "$LOGS/php-buildconf.log"; die "buildconf 失败"; }
fi

log "configure（日志 ${LOGS#"$PHPRT_HOME"/}/php-configure.log）"
CPPFLAGS="${CPPFLAGS% }" LDFLAGS="$LDFLAGS" \
  ./configure "${ARGS[@]}" > "$LOGS/php-configure.log" 2>&1 \
  || { tail -40 "$LOGS/php-configure.log"; die "configure 失败，日志: ${LOGS#"$PHPRT_HOME"/}/php-configure.log"; }
ok "configure 完成"

log "make -j$JOBS（日志 ${LOGS#"$PHPRT_HOME"/}/php-make.log）"
make -j"$JOBS" > "$LOGS/php-make.log" 2>&1 \
  || { tail -40 "$LOGS/php-make.log"; die "make 失败，日志: ${LOGS#"$PHPRT_HOME"/}/php-make.log"; }

log "安装到 ${PREFIX#"$PHPRT_HOME"/}"
make install > "$LOGS/php-install.log" 2>&1 \
  || { tail -40 "$LOGS/php-install.log"; die "make install 失败"; }

# ------------------------------------------------------- 依赖记录 + ini ------
# 记录本 PHP 实际链接的依赖目录：跳过判断与 --prune 都靠它
# （同一个依赖可能同时有 lib 和 lib64，按目录名去重）
for d in ${LIB_DIRS[@]+"${LIB_DIRS[@]}"}; do
  printf '%s\n' "$(dirname "$d")"
done | awk '!seen[$0]++' > "$PREFIX/.phprt-deps"

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir 2>/dev/null || true)"
mkdir -p "$PREFIX/etc/conf.d" "$EXT_DIR"

# 自编译的 OpenSSL 不带系统 CA 目录，https 会报 certificate verify failed。
# 找一个系统 CA 包写进 php.ini（找不到就注释掉，用户可自行改）。
CA=""
for f in /etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem \
         /etc/ssl/certs/ca-certificates.crt /etc/ssl/cert.pem; do
  [ -f "$f" ] && { CA="$f"; break; }
done
CA_LINES="; 没找到系统 CA 包，需要 https 时手动设置 openssl.cafile"
[ -n "$CA" ] && CA_LINES="openssl.cafile = \"$CA\"
curl.cainfo = \"$CA\""

cat > "$PREFIX/etc/php.ini" <<EOF
; 由 phprt 生成（版本与开关见安装根 configs/php-<版本>.conf）
[PHP]
date.timezone = Asia/Shanghai
memory_limit = 256M
error_reporting = E_ALL
display_errors = On
display_startup_errors = On
log_errors = On
max_execution_time = 0
default_charset = "UTF-8"
extension_dir = "$EXT_DIR"
$CA_LINES
; FFI：CLI 里直接 FFI::cdef 需要 1（生产可改 preload）
ffi.enable = 1

[opcache]
opcache.enable = 1
opcache.enable_cli = 1
EOF

# 扩展只写文件名：扩展目录由 php.ini / myphp 给出，整个安装根搬走也有效
entries=""
for ext in openssl curl sqlite3 pdo_sqlite mysqli pdo_mysql zip intl; do
  [ -f "$EXT_DIR/$ext.so" ] && entries+="extension=$ext.so"$'\n'
done
[ -f "$EXT_DIR/opcache.so" ] && entries+="zend_extension=opcache.so"$'\n'
if [ -n "$entries" ]; then
  printf '%s' "$entries" > "$PREFIX/etc/conf.d/10-extensions.ini"
else
  rm -f "$PREFIX/etc/conf.d/10-extensions.ini"
fi

stamp_write php "$SIG"
ok "PHP $("$PREFIX/bin/php" -r 'echo PHP_VERSION;') 安装完成 -> $PREFIX"
