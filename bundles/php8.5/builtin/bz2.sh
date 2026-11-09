#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=bz2 switch=ENABLE_BZ2 modules=bz2 ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_BZ2:-0}" = 1 ]; then
    printf '%s\n' "--with-bz2=$(dep_dir_of bzip2)"
  else
    printf '%s\n' --without-bz2
  fi
}

needs() {
  [ "${ENABLE_BZ2:-0}" = 1 ] && printf '%s\n' bzip2
  return 0
}

recipe_main "$@"
