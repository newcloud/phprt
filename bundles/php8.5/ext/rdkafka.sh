#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=rdkafka switch=ENABLE_RDKAFKA version=RDKAFKA_VERSION modules=rdkafka ini=20-rdkafka.ini tools=autoconf,perlshim needs=librdkafka optional= upstream=pecl:rdkafka
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  dep_env rdkafka
  ext_pecl_build rdkafka "$RDKAFKA_VERSION" "$RDKAFKA_URL" "$RDKAFKA_SHA256" \
    --with-rdkafka="$(dep_dir_of librdkafka)"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-rdkafka.ini" <<INI
; 由 phprt 生成（rdkafka $RDKAFKA_VERSION / librdkafka $LIBRDKAFKA_VERSION）
extension=rdkafka.so
INI
}

recipe_main "$@"
