#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=redis switch=ENABLE_REDIS version=REDIS_VERSION modules=redis ini=20-redis.ini tools=autoconf,perlshim needs= optional= upstream=pecl:redis type=static,shared default=shared category=CacheStorage
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# serializer 全部用 redis 自带实现，不引入 igbinary/msgpack/zstd。
# lzf 用 redis 自带的 liblzf，不新增外部依赖。
opts() {
  printf '%s\t%s\t%s\n' \
    lzf off 'LZF 压缩（自带 FastLZ）'
}

configure_args() {
  printf '%s\n' --enable-redis
  printf '%s\n' --enable-redis-session
  printf '%s\n' --enable-redis-json
  [ "${ENABLE_REDIS_LZF:-0}" = 1 ] && printf '%s\n' --enable-redis-lzf
}

build() {
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_pecl_build redis "$REDIS_VERSION" "$REDIS_URL" "$REDIS_SHA256" "${_args[@]}"
}

stage_static() {
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_stage_static redis "$REDIS_VERSION" "$REDIS_URL" "$REDIS_SHA256" "${_args[@]}"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-redis.ini" <<INI
; 由 phprt 生成（redis $REDIS_VERSION）
extension=redis.so
INI
}

recipe_main "$@"
