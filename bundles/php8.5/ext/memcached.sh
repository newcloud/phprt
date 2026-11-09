#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=memcached switch=ENABLE_MEMCACHED version=MEMCACHED_VERSION modules=memcached ini=20-memcached.ini tools=autoconf,perlshim needs=libmemcached,zlib optional= upstream=pecl:memcached
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  dep_env memcached
  ext_pecl_build memcached "$MEMCACHED_VERSION" "$MEMCACHED_URL" "$MEMCACHED_SHA256" \
    --with-libmemcached-dir="$(dep_dir_of libmemcached)" \
    --with-zlib-dir="$(dep_dir_of zlib)" \
    --disable-memcached-sasl \
    --enable-memcached-session \
    --enable-memcached-json
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-memcached.ini" <<INI
; 由 phprt 生成（memcached $MEMCACHED_VERSION / libmemcached $LIBMEMCACHED_VERSION）
extension=memcached.so
INI
}

recipe_main "$@"
