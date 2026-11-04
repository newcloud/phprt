#!/usr/bin/env bash
# =============================================================================
#  内置扩展：zip（依赖 libzip）
#  phprt: builtin=zip switch=ENABLE_LIBZIP
#  由 php.sh 调用，不单独构建：
#    --flags  输出 php configure 参数（一行一个；开关关闭时输出负向参数）
#    --needs  输出依赖的库（空格分隔，随开关变化）
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    # zip 默认关闭，不需要负向参数
    if [ "${ENABLE_LIBZIP:-0}" = 1 ]; then
      printf '%s\n' "--with-zip=$(dep_dir_of libzip)"
    fi ;;
  --needs)
    if [ "${ENABLE_LIBZIP:-0}" = 1 ]; then printf '%s\n' libzip; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
