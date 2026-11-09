#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=zlib switch=ENABLE_ZLIB modules=zlib ini=20-zlib.ini
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  # 共享模块，便于按项目启用/停用
  if [ "${ENABLE_ZLIB:-0}" = 1 ]; then
    printf '%s\n' "--with-zlib=shared"
  else
    printf '%s\n' --without-zlib
  fi
}

needs() {
  [ "${ENABLE_ZLIB:-0}" = 1 ] && printf '%s\n' zlib
  return 0
}

recipe_main "$@"
