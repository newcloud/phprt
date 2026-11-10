#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=zlib switch=ENABLE_ZLIB modules=zlib ini=- type=static default=static category=Compression
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_ZLIB:-0}" = 1 ]; then
    # phar 的 gzip 依赖编译期 zlib，所以只能是 static
    printf '%s\n' "--with-zlib=$(dep_dir_of zlib)"
  else
    printf '%s\n' --without-zlib
  fi
}

needs() {
  [ "${ENABLE_ZLIB:-0}" = 1 ] && printf '%s\n' zlib
  return 0
}

recipe_main "$@"
