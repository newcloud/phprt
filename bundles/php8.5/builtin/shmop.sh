#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=shmop switch=ENABLE_SHMOP modules=shmop ini=- type=static,shared default=static category=Other
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_SHMOP:-0}" = 1 ]; then
    printf '%s\n' --enable-shmop
  else
    printf '%s\n' --disable-shmop
  fi
}

needs() { :; }

recipe_main "$@"
