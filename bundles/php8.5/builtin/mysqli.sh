#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=mysqli switch=ENABLE_MYSQLI modules=mysqli,pdo_mysql ini=- type=static default=static
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_MYSQLI:-0}" = 1 ]; then
    printf '%s\n' --with-mysqli=mysqlnd --with-pdo-mysql=mysqlnd
  else
    printf '%s\n' --without-mysqli --without-pdo-mysql
  fi
}

needs() { :; }

recipe_main "$@"
