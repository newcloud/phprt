#!/usr/bin/env bash
# =============================================================================
#  内置扩展：SQLite（sqlite3 / pdo_sqlite）
#  phprt: builtin=sqlite switch=ENABLE_SQLITE
#  由 php.sh 调用，不单独构建：
#    --flags  输出 php configure 参数（一行一个；开关关闭时输出负向参数）
#    --needs  输出依赖的库（空格分隔，随开关变化）
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_SQLITE:-0}" = 1 ]; then
      d="$(dep_dir_of sqlite)"
      printf '%s\n' "--with-sqlite3=$d" "--with-pdo-sqlite=$d"
    else
      printf '%s\n' --without-sqlite3 --without-pdo-sqlite
    fi ;;
  --needs)
    if [ "${ENABLE_SQLITE:-0}" = 1 ]; then printf '%s\n' sqlite; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
