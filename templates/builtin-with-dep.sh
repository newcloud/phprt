#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=__NAME__ switch=ENABLE___UPPER__ modules=__NAME__ ini=- type=static,shared default=static category=__CATEGORY__ needs=__DEP__ note=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE___UPPER__:-0}" = 1 ]; then
    printf '%s\n' "--with-__NAME__=$(dep_dir_of __DEP__)"
  else
    printf '%s\n' --without-__NAME__
  fi
}

needs() {
  [ "${ENABLE___UPPER__:-0}" = 1 ] && printf '%s\n' __DEP__
  return 0
}

recipe_main "$@"
