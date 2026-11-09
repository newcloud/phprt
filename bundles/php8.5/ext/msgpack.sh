#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=msgpack switch=ENABLE_MSGPACK version=MSGPACK_VERSION modules=msgpack ini=20-msgpack.ini tools=autoconf,perlshim needs= optional= upstream=pecl:msgpack
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# 源码自带 bundled msgpack-c，不需要外部库
build() {
  ext_pecl_build msgpack "$MSGPACK_VERSION" "$MSGPACK_URL" "$MSGPACK_SHA256" --with-msgpack
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-msgpack.ini" <<INI
; 由 phprt 生成（msgpack $MSGPACK_VERSION）
extension=msgpack.so
INI
}

recipe_main "$@"
