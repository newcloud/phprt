#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=gmp switch=ENABLE_GMP modules=gmp ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_GMP:-0}" = 1 ]; then
    printf '%s\n' "--with-gmp=$(dep_dir_of gmp)"
  else
    printf '%s\n' --without-gmp
  fi
}

needs() {
  [ "${ENABLE_GMP:-0}" = 1 ] && printf '%s\n' gmp
  return 0
}

recipe_main "$@"
