#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=pgsql switch=ENABLE_PGSQL modules=pgsql,pdo_pgsql ini=- type=static default=static
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# PHP 用 pg_config 定位 libpq；DIR 是安装前缀，它自己会找 bin/pg_config
flags() {
  if [ "${ENABLE_PGSQL:-0}" = 1 ]; then
    local d
    d="$(dep_dir_of libpq)"
    printf '%s\n' "--with-pgsql=$d" "--with-pdo-pgsql=$d"
  else
    printf '%s\n' --without-pgsql --without-pdo-pgsql
  fi
}

needs() {
  [ "${ENABLE_PGSQL:-0}" = 1 ] && printf '%s\n' libpq
  return 0
}

recipe_main "$@"
