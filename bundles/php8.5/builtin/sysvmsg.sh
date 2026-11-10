#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=sysvmsg switch=ENABLE_SYSVMSG modules=sysvmsg ini=- type=static,shared default=static category=Other
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_SYSVMSG:-0}" = 1 ]; then
    printf '%s\n' --enable-sysvmsg
  else
    printf '%s\n' --disable-sysvmsg
  fi
}

needs() { :; }

recipe_main "$@"
