#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=zip switch=ENABLE_LIBZIP modules=zip ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  # zip 默认不编译，不需要负向参数
  if [ "${ENABLE_LIBZIP:-0}" = 1 ]; then
    printf '%s\n' "--with-zip=$(dep_dir_of libzip)"
  fi
}

needs() {
  [ "${ENABLE_LIBZIP:-0}" = 1 ] && printf '%s\n' libzip
}

recipe_main "$@"
