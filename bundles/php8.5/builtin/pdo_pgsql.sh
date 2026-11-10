#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=pdo_pgsql switch=ENABLE_PDO_PGSQL modules=pdo_pgsql ini=- type=static default=static category=Database menu_order=30
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# PDO 的 PostgreSQL 驱动。和 pgsql 共用同一份 libpq（固定带 TLS）。
# PDO 本体默认开启，不需要 --enable-pdo。
flags() {
  if [ "${ENABLE_PDO_PGSQL:-0}" = 1 ]; then
    printf '%s\n' "--with-pdo-pgsql=$(dep_dir_of libpq)"
  else
    printf '%s\n' --without-pdo-pgsql
  fi
}

needs() {
  [ "${ENABLE_PDO_PGSQL:-0}" = 1 ] && printf '%s\n' libpq
  return 0
}

recipe_main "$@"
