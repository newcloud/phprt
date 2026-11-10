#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=mbstring switch=ENABLE_ONIGURUMA modules=mbstring ini=- type=static,shared default=static category=Internationalization
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_ONIGURUMA:-0}" = 1 ]; then
    printf '%s\n' --enable-mbstring
  else
    printf '%s\n' --disable-mbstring
  fi
}

needs() {
  [ "${ENABLE_ONIGURUMA:-0}" = 1 ] && printf '%s\n' oniguruma
  return 0
}

recipe_main "$@"
