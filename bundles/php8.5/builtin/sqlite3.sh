#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=sqlite3 switch=ENABLE_SQLITE3 modules=sqlite3,pdo_sqlite ini=- type=static default=static category=Database menu_order=50
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# SQLite 只保留 sqlite3 一个条目，同一个 recipe 一起产出 sqlite3 和 pdo_sqlite 模块
# （两者共用同一份 SQLite 3，不能重复构建）。
flags() {
  if [ "${ENABLE_SQLITE3:-0}" = 1 ]; then
    local d
    d="$(dep_dir_of sqlite)"
    printf '%s\n' "--with-sqlite3=$d" "--with-pdo-sqlite=$d"
  else
    printf '%s\n' --without-sqlite3 --without-pdo-sqlite
  fi
}

needs() {
  [ "${ENABLE_SQLITE3:-0}" = 1 ] && printf '%s\n' sqlite
  return 0
}

recipe_main "$@"
