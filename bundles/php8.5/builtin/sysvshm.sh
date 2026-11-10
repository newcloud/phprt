#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=sysvshm switch=ENABLE_SYSVSHM modules=sysvshm ini=- type=static,shared default=static category=Other
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_SYSVSHM:-0}" = 1 ]; then
    printf '%s\n' --enable-sysvshm
  else
    printf '%s\n' --disable-sysvshm
  fi
}

needs() { :; }

recipe_main "$@"
