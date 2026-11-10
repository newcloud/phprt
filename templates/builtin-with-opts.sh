#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=__NAME__ switch=ENABLE___UPPER__ modules=__NAME__ ini=- type=static,shared default=static category=__CATEGORY__ needs=__DEP__ note=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# key<TAB>state<TAB>label；state = on / off / fixed
opts() {
  printf '%s\t%s\t%s\n' \
    __OPT1__ on  '__OPT1_LABEL__' \
    __OPT2__ off '__OPT2_LABEL__'
}

flags() {
  if [ "${ENABLE___UPPER__:-0}" = 1 ]; then
    printf '%s\n' --enable-__NAME__
    [ "${ENABLE___UPPER_____OPT1__:-0}" = 1 ] &&
      printf '%s\n' "--with-__OPT1__=$(dep_dir_of __DEP__)"
    [ "${ENABLE___UPPER_____OPT2__:-0}" = 1 ] &&
      printf '%s\n' "--with-__OPT2__=$(dep_dir_of __DEP__)"
  else
    printf '%s\n' --disable-__NAME__
  fi
}

needs() {
  [ "${ENABLE___UPPER__:-0}" = 1 ] || return 0
  [ "${ENABLE___UPPER_____OPT1__:-0}" = 1 ] && printf '%s\n' __DEP__
  [ "${ENABLE___UPPER_____OPT2__:-0}" = 1 ] && printf '%s\n' __DEP__
  return 0
}

recipe_main "$@"
