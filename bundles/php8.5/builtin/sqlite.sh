#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=sqlite switch=ENABLE_SQLITE modules=sqlite3,pdo_sqlite ini=- type=static default=static
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_SQLITE:-0}" = 1 ]; then
    local d
    d="$(dep_dir_of sqlite)"
    printf '%s\n' "--with-sqlite3=$d" "--with-pdo-sqlite=$d"
  else
    printf '%s\n' --without-sqlite3 --without-pdo-sqlite
  fi
}

needs() {
  [ "${ENABLE_SQLITE:-0}" = 1 ] && printf '%s\n' sqlite
  return 0
}

recipe_main "$@"
