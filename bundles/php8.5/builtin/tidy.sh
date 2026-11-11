#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=tidy switch=ENABLE_TIDY modules=tidy ini=- type=static,shared default=static category=XML
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_TIDY:-0}" = 1 ]; then
    printf '%s\n' "--with-tidy=$(dep_dir_of tidy)"
  else
    printf '%s\n' --without-tidy
  fi
}

needs() {
  [ "${ENABLE_TIDY:-0}" = 1 ] && printf '%s\n' tidy
  return 0
}

recipe_main "$@"
