#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=rdkafka switch=ENABLE_RDKAFKA version=RDKAFKA_VERSION modules=rdkafka ini=20-rdkafka.ini tools=autoconf,perlshim needs=librdkafka optional= upstream=pecl:rdkafka type=static,shared default=shared category=Messaging
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

configure_args() {
  printf '%s\n' --with-rdkafka="$(dep_dir_of librdkafka)"
}

build() {
  dep_env rdkafka
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_pecl_build rdkafka "$RDKAFKA_VERSION" "$RDKAFKA_URL" "$RDKAFKA_SHA256" "${_args[@]}"
}

stage_static() {
  dep_env rdkafka
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_stage_static rdkafka "$RDKAFKA_VERSION" "$RDKAFKA_URL" "$RDKAFKA_SHA256" "${_args[@]}"
}

static_ldflags() {
  printf '%s\n' -lrdkafka -lstdc++
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-rdkafka.ini" <<INI
; 由 phprt 生成（rdkafka $RDKAFKA_VERSION / librdkafka $LIBRDKAFKA_VERSION）
extension=rdkafka.so
INI
}

recipe_main "$@"
