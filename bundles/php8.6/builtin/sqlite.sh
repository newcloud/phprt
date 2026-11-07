#!/usr/bin/env bash
# 内置扩展：SQLite（shared：shared，便于按项目启用/停用）
#  phprt: builtin=sqlite switch=ENABLE_SQLITE
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_SQLITE:-0}" = 1 ]; then
      printf '%s\n' "--with-sqlite3=shared" "--with-pdo-sqlite=shared"
    else
      printf '%s\n' --without-sqlite3 --without-pdo-sqlite
    fi ;;
  --needs)
    if [ "${ENABLE_SQLITE:-0}" = 1 ]; then printf '%s\n' sqlite; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
