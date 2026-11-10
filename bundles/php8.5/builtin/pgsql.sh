#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=pgsql switch=ENABLE_PGSQL modules=pgsql ini=- type=static default=static category=Database menu_order=40
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# PostgreSQL 专用扩展（非 PDO）。libpq 固定带 TLS，不再提供 ssl 子项。
# PHP 用 pg_config 定位 libpq；DIR 是安装前缀，它自己会找 bin/pg_config。
flags() {
  if [ "${ENABLE_PGSQL:-0}" = 1 ]; then
    printf '%s\n' "--with-pgsql=$(dep_dir_of libpq)"
  else
    printf '%s\n' --without-pgsql
  fi
}

needs() {
  [ "${ENABLE_PGSQL:-0}" = 1 ] && printf '%s\n' libpq
  return 0
}

recipe_main "$@"
