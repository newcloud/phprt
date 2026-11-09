#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=redis switch=ENABLE_REDIS version=REDIS_VERSION modules=redis ini=20-redis.ini tools=autoconf,perlshim needs= optional= upstream=pecl:redis
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# serializer 全部用 redis 自带实现，不引入 igbinary/msgpack/zstd
build() {
  ext_pecl_build redis "$REDIS_VERSION" "$REDIS_URL" "$REDIS_SHA256" \
    --enable-redis --enable-redis-session --enable-redis-json
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-redis.ini" <<INI
; 由 phprt 生成（redis $REDIS_VERSION）
extension=redis.so
INI
}

recipe_main "$@"
