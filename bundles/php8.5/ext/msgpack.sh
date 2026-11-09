#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=msgpack switch=ENABLE_MSGPACK version=MSGPACK_VERSION modules=msgpack ini=20-msgpack.ini tools=autoconf,perlshim needs= optional= upstream=pecl:msgpack type=static,shared default=shared
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# 源码自带 bundled msgpack-c，不需要外部库
configure_args() {
  printf '%s\n' --with-msgpack
}

build() {
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_pecl_build msgpack "$MSGPACK_VERSION" "$MSGPACK_URL" "$MSGPACK_SHA256" "${_args[@]}"
}

stage_static() {
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_stage_static msgpack "$MSGPACK_VERSION" "$MSGPACK_URL" "$MSGPACK_SHA256" "${_args[@]}"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-msgpack.ini" <<INI
; 由 phprt 生成（msgpack $MSGPACK_VERSION）
extension=msgpack.so
INI
}

recipe_main "$@"
