#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=amqp switch=ENABLE_AMQP version=AMQP_VERSION modules=amqp ini=20-amqp.ini tools=autoconf,perlshim needs=rabbitmq-c optional= upstream=pecl:amqp type=static,shared default=shared category=Network
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

configure_args() {
  printf '%s\n' --with-amqp
  printf '%s\n' --with-librabbitmq-dir="$(dep_dir_of rabbitmq-c)"
}

build() {
  dep_env amqp
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_pecl_build amqp "$AMQP_VERSION" "$AMQP_URL" "$AMQP_SHA256" "${_args[@]}"
}

stage_static() {
  dep_env amqp
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_stage_static amqp "$AMQP_VERSION" "$AMQP_URL" "$AMQP_SHA256" "${_args[@]}"
}

static_ldflags() {
  printf '%s\n' -lrabbitmq
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-amqp.ini" <<INI
; 由 phprt 生成（amqp $AMQP_VERSION / rabbitmq-c $RABBITMQ_C_VERSION）
extension=amqp.so
INI
}

recipe_main "$@"
