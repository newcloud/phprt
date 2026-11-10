#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=sockets switch=ENABLE_SOCKETS modules=sockets ini=- type=static,shared default=static category=Network
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_SOCKETS:-0}" = 1 ]; then
    printf '%s\n' --enable-sockets
  else
    printf '%s\n' --disable-sockets
  fi
}

needs() { :; }

recipe_main "$@"
