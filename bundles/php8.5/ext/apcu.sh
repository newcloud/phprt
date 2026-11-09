#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=apcu switch=ENABLE_APCU version=APCU_VERSION modules=apcu ini=25-apcu.ini tools=autoconf,perlshim needs= optional= upstream=pecl:apcu type=static,shared default=shared
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

configure_args() {
  printf '%s\n' --enable-apcu
}

build() {
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_pecl_build apcu "$APCU_VERSION" "$APCU_URL" "$APCU_SHA256" "${_args[@]}"
}

stage_static() {
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_stage_static apcu "$APCU_VERSION" "$APCU_URL" "$APCU_SHA256" "${_args[@]}"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/25-apcu.ini" <<INI
; 由 phprt 生成（apcu $APCU_VERSION）
extension=apcu.so
apc.enabled=1
apc.enable_cli=1
apc.shm_size=32M
apc.ttl=0
INI
}

recipe_main "$@"
