#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=curl switch=ENABLE_LIBCURL modules=curl ini=- type=static,shared default=static
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_LIBCURL:-0}" = 1 ]; then
    printf '%s\n' "--with-curl=$(dep_dir_of curl)"
  else
    printf '%s\n' --without-curl
  fi
}

needs() {
  [ "${ENABLE_LIBCURL:-0}" = 1 ] && printf '%s\n' curl
}

recipe_main "$@"
