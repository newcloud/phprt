#!/usr/bin/env bash
# =============================================================================
#  内置扩展：zlib（gz 函数、压缩 phar）
#  phprt: builtin=zlib switch=ENABLE_ZLIB
#  由 php.sh 调用，不单独构建：
#    --flags  输出 php configure 参数（一行一个；开关关闭时输出负向参数）
#    --needs  输出依赖的库（空格分隔，随开关变化）
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_ZLIB:-0}" = 1 ]; then
      printf '%s\n' "--with-zlib=$(dep_dir_of zlib)"
    else
      printf '%s\n' "--without-zlib"
    fi ;;
  --needs)
    if [ "${ENABLE_ZLIB:-0}" = 1 ]; then printf '%s\n' zlib; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
