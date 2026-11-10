#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=mysqli switch=ENABLE_MYSQLI modules=mysqli ini=- type=static default=static category=Database menu_order=10
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# MySQL 专用扩展（非 PDO）。mysqlnd 由 PHP 在 --with-mysqli=mysqlnd 时隐式编入，
# 不是独立依赖，也不出现在扩展清单里。
flags() {
  if [ "${ENABLE_MYSQLI:-0}" = 1 ]; then
    printf '%s\n' --with-mysqli=mysqlnd
  else
    printf '%s\n' --without-mysqli
  fi
}

needs() {
  # mysqlnd 的压缩协议要用 zlib；用树内的那份，避免链到系统 libz。
  [ "${ENABLE_MYSQLI:-0}" = 1 ] && printf '%s\n' zlib
  return 0
}

recipe_main "$@"
