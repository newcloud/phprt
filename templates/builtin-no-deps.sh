#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=__NAME__ switch=ENABLE___UPPER__ modules=__NAME__ ini=- type=static,shared default=static category=__CATEGORY__ needs= note=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE___UPPER__:-0}" = 1 ]; then
    printf '%s\n' --enable-__NAME__
  else
    printf '%s\n' --disable-__NAME__
  fi
}

needs() {
  return 0
}

recipe_main "$@"
