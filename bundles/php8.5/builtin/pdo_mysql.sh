#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=pdo_mysql switch=ENABLE_PDO_MYSQL modules=pdo_mysql ini=- type=static default=static category=Database menu_order=20
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# PDO 的 MySQL 驱动。PDO 本体 PHP 8.5 默认开启，不需要 --enable-pdo；
# mysqlnd 由 --with-pdo-mysql=mysqlnd 隐式编入。
flags() {
  if [ "${ENABLE_PDO_MYSQL:-0}" = 1 ]; then
    printf '%s\n' --with-pdo-mysql=mysqlnd
  else
    printf '%s\n' --without-pdo-mysql
  fi
}

needs() {
  # mysqlnd 的压缩协议要用 zlib；用树内的那份，避免链到系统 libz。
  [ "${ENABLE_PDO_MYSQL:-0}" = 1 ] && printf '%s\n' zlib
  return 0
}

recipe_main "$@"
