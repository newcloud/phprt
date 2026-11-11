#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=xdebug switch=ENABLE_XDEBUG version=XDEBUG_VERSION modules=xdebug ini=30-xdebug.ini tools=autoconf,perlshim needs=zlib optional= upstream=pecl:xdebug type=shared default=shared category=Other
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# profiler 压缩走 pkg-config 找 zlib；不注入会拿系统的
build() {
  dep_env xdebug
  ext_pecl_build xdebug "$XDEBUG_VERSION" "$XDEBUG_URL" "$XDEBUG_SHA256"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/30-xdebug.ini" <<INI
; 由 phprt 生成（xdebug $XDEBUG_VERSION）
; 默认关掉，不拖慢 worker；调试时 export XDEBUG_MODE=debug
zend_extension=xdebug.so
xdebug.mode=off
xdebug.start_with_request=yes
xdebug.client_host=127.0.0.1
xdebug.client_port=9003
INI
}

recipe_main "$@"
