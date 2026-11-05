#!/usr/bin/env bash
# =============================================================================
#  libcurl（PHP curl 扩展；http/https/ftp + dict，TLS 用本地 OpenSSL）
#  安装到: deps/curl-<版本>/
#  可单独执行: lib/modules/deps/curl.sh
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of curl)"
SIG="curl=$LIBCURL_VERSION;openssl=${ENABLE_OPENSSL:-0}:${OPENSSL_VERSION:-none};zlib=${ENABLE_ZLIB:-0}:${ZLIB_VERSION:-none};dict=1"

if [ "$FORCE" != 1 ] && stamp_ok curl "$SIG" && [ -f "$D/lib/libcurl.so" ]; then
  ok "libcurl 已安装，跳过"; exit 0
fi

ensure_clean_target curl "$D"

TAR="$(basename "$LIBCURL_URL")"
fetch "$LIBCURL_URL" "$DOWNLOADS/$TAR" "${LIBCURL_SHA256:-}"

SRC="$SRCDIR/curl-$LIBCURL_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
  ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
else
  rm -rf "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRCDIR"
  [ -d "$SRC" ] || die "解压后未找到 $SRC"
fi

# 只保留 http/https/ftp 等常用协议；DICT 保留以对齐发行版
OPTS=(
  --without-libpsl --without-brotli --without-zstd --without-libidn2
  --without-libssh2 --without-nghttp2 --without-nghttp3
  --disable-ldap --disable-ldaps
  --disable-rtsp --enable-dict --disable-telnet --disable-tftp
  --disable-pop3 --disable-imap --disable-smtp --disable-gopher
  --disable-mqtt --disable-manual
  --enable-shared --enable-static
)
ZLIB_D="$(dep_dir_of zlib)"
OPENSSL_D="$(dep_dir_of openssl)"
[ "${ENABLE_OPENSSL:-0}" = 1 ] && OPTS+=(--with-openssl="$OPENSSL_D") || OPTS+=(--without-openssl)
[ "${ENABLE_ZLIB:-0}" = 1 ]    && OPTS+=(--with-zlib="$ZLIB_D")       || OPTS+=(--without-zlib)

cd "$SRC"
log "configure + make -j$JOBS"
PKG_CONFIG_PATH="$(dep_pc_path openssl zlib)${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" \
CPPFLAGS="$(dep_include_flags openssl zlib)" \
LDFLAGS="-L$OPENSSL_D/lib -L$ZLIB_D/lib -Wl,-rpath,$OPENSSL_D/lib:$ZLIB_D/lib -Wl,--enable-new-dtags" \
  ./configure --prefix="$D" "${OPTS[@]}" > "$LOGS/libcurl-configure.log" 2>&1 \
  || { tail -30 "$LOGS/libcurl-configure.log"; die "libcurl configure 失败"; }
make -j"$JOBS" > "$LOGS/libcurl-make.log" 2>&1 \
  || { tail -30 "$LOGS/libcurl-make.log"; die "libcurl make 失败"; }
make install >> "$LOGS/libcurl-make.log" 2>&1

stamp_write curl "$SIG"
ok "libcurl $LIBCURL_VERSION -> $D"
