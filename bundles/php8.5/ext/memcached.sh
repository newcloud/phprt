#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=memcached switch=ENABLE_MEMCACHED version=MEMCACHED_VERSION modules=memcached ini=20-memcached.ini tools=autoconf,perlshim needs=libmemcached,zlib optional= upstream=pecl:memcached type=static,shared default=shared category=CacheStorage
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

configure_args() {
  printf '%s\n' --enable-memcached
  printf '%s\n' --with-libmemcached-dir="$(dep_dir_of libmemcached)"
  printf '%s\n' --with-zlib-dir="$(dep_dir_of zlib)"
  printf '%s\n' --disable-memcached-sasl
  printf '%s\n' --enable-memcached-session
  printf '%s\n' --enable-memcached-json
}

build() {
  dep_env memcached
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_pecl_build memcached "$MEMCACHED_VERSION" "$MEMCACHED_URL" "$MEMCACHED_SHA256" "${_args[@]}"
}

stage_static() {
  dep_env memcached
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_stage_static memcached "$MEMCACHED_VERSION" "$MEMCACHED_URL" "$MEMCACHED_SHA256" "${_args[@]}"
}

static_ldflags() {
  printf '%s\n' -lmemcached -lz
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-memcached.ini" <<INI
; 由 phprt 生成（memcached $MEMCACHED_VERSION / libmemcached $LIBMEMCACHED_VERSION）
extension=memcached.so
INI
}

recipe_main "$@"
