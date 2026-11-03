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

SIG="$(php_sig)"

if [ "$FORCE" != 1 ] && [ "$REBUILD" != 1 ] && php_is_current; then
  ok "PHP 已安装且编译配置未变（--rebuild 可重编）"
  exit 0
fi

ensure_clean_target php "$PREFIX"

TAR="$(basename "$PHP_URL")"
fetch "$PHP_URL" "$DOWNLOADS/$TAR" "${PHP_SHA256:-}"

SRC="$SRCDIR/php-$PHP_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码目录 $SRC"
  ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  log "解压 $TAR -> build/"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

D_OPENSSL="$(dep_dir_of openssl)"
D_ZLIB="$(dep_dir_of zlib)"
D_LIBXML2="$(dep_dir_of libxml2)"
D_SQLITE="$(dep_dir_of sqlite)"
D_CURL="$(dep_dir_of curl)"
D_ONIG="$(dep_dir_of oniguruma)"
D_ICU="$(dep_dir_of icu)"
D_LIBZIP="$(dep_dir_of libzip)"

# ------------------------------------------------------------------ 参数 ----
ARGS=(
  --prefix="$PREFIX"
  --with-config-file-path="$PREFIX/etc"
  --with-config-file-scan-dir="$PREFIX/etc/conf.d"
  --enable-cli --disable-cgi
  --without-pear
  --enable-phar --enable-filter --enable-fileinfo
  --enable-opcache
)

if [ "${ENABLE_OPENSSL:-0}" = 1 ]; then ARGS+=(--with-openssl="$D_OPENSSL"); else ARGS+=(--without-openssl); fi
if [ "${ENABLE_ZLIB:-0}" = 1 ]; then ARGS+=(--with-zlib="$D_ZLIB"); else ARGS+=(--without-zlib); fi
if [ "${ENABLE_LIBXML2:-0}" = 1 ]; then
  ARGS+=(--with-libxml --enable-dom --enable-xml --enable-simplexml --enable-xmlreader --enable-xmlwriter --enable-soap)
else
  ARGS+=(--without-libxml --disable-dom --disable-xml --disable-simplexml \
         --disable-xmlreader --disable-xmlwriter --disable-soap)
fi
if [ "${ENABLE_LIBCURL:-0}" = 1 ]; then ARGS+=(--with-curl="$D_CURL"); else ARGS+=(--without-curl); fi
if [ "${ENABLE_SQLITE:-0}" = 1 ]; then
  ARGS+=(--with-sqlite3="$D_SQLITE" --with-pdo-sqlite="$D_SQLITE")
else
  ARGS+=(--without-sqlite3 --without-pdo-sqlite)
fi
ARGS+=(--without-mysqli --without-pdo-mysql)   # MySQL 相关暂不启用
[ "${ENABLE_ONIGURUMA:-0}" = 1 ] && ARGS+=(--enable-mbstring)
[ "${ENABLE_ICU:-0}" = 1 ] && ARGS+=(--enable-intl --with-icu-dir="$D_ICU")
[ "${ENABLE_LIBZIP:-0}" = 1 ] && ARGS+=(--with-zip="$D_LIBZIP")

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
RPATH=""
PC_PATH=""
for d in ${LIB_DIRS[@]+"${LIB_DIRS[@]}"}; do
  LDFLAGS+="-L$d "
  RPATH+="$d:"
  CPPFLAGS+="-I$(dirname "$d")/include "
  [ -d "$d/pkgconfig" ] && PC_PATH+="$d/pkgconfig:"
done
RPATH="${RPATH%:}"
export PKG_CONFIG_PATH="${PC_PATH%:}${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
[ -n "$RPATH" ] && LDFLAGS+="-Wl,-rpath,$RPATH -Wl,--enable-new-dtags"

# ------------------------------------------------------------------ 编译 ----
cd "$SRC"
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
: > "$PREFIX/.phprt-deps"
for d in ${LIB_DIRS[@]+"${LIB_DIRS[@]}"}; do
  printf '%s\n' "$(dirname "$d")" >> "$PREFIX/.phprt-deps"
done

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
; 由 phprt 生成（版本与开关见 phprt/config.conf）
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
