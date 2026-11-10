#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=sysvsem switch=ENABLE_SYSVSEM modules=sysvsem ini=- type=static,shared default=static category=Other
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_SYSVSEM:-0}" = 1 ]; then
    printf '%s\n' --enable-sysvsem
  else
    printf '%s\n' --disable-sysvsem
  fi
}

needs() { :; }

recipe_main "$@"
