#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=protobuf switch=ENABLE_PROTOBUF version=PROTOBUF_VERSION modules=protobuf ini=23-protobuf.ini tools=autoconf,perlshim needs= optional= upstream=pecl:protobuf
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# 纯 C，自带精简版 upb，不依赖外部 libprotobuf
build() {
  ext_pecl_build protobuf "$PROTOBUF_VERSION" "$PROTOBUF_URL" "$PROTOBUF_SHA256" \
    --enable-protobuf
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/23-protobuf.ini" <<INI
; 由 phprt 生成（protobuf $PROTOBUF_VERSION）
extension=protobuf.so
INI
}

recipe_main "$@"
