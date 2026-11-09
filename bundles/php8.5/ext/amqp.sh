#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=amqp switch=ENABLE_AMQP version=AMQP_VERSION modules=amqp ini=20-amqp.ini tools=autoconf,perlshim needs=rabbitmq-c optional= upstream=pecl:amqp
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  dep_env amqp
  ext_pecl_build amqp "$AMQP_VERSION" "$AMQP_URL" "$AMQP_SHA256" \
    --with-amqp --with-librabbitmq-dir="$(dep_dir_of rabbitmq-c)"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-amqp.ini" <<INI
; 由 phprt 生成（amqp $AMQP_VERSION / rabbitmq-c $RABBITMQ_C_VERSION）
extension=amqp.so
INI
}

recipe_main "$@"
