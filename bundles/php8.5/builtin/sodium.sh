#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=sodium switch=ENABLE_SODIUM modules=sodium ini=- type=static,shared default=static category=Cryptography
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_SODIUM:-0}" = 1 ]; then
    printf '%s\n' "--with-sodium=$(dep_dir_of libsodium)"
  else
    printf '%s\n' --without-sodium
  fi
}

needs() {
  [ "${ENABLE_SODIUM:-0}" = 1 ] && printf '%s\n' libsodium
  return 0
}

recipe_main "$@"
