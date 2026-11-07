#!/usr/bin/env bash
# =============================================================================
#  内置扩展：mbstring（正则部分依赖 oniguruma）
#  phprt: builtin=mbstring switch=ENABLE_ONIGURUMA
#  由 php.sh 调用，不单独构建：
#    --flags  输出 php configure 参数（一行一个；开关关闭时输出负向参数）
#    --needs  输出依赖的库（空格分隔，随开关变化）
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    # mbstring 默认关闭，开关关掉时不需要负向参数
    if [ "${ENABLE_ONIGURUMA:-0}" = 1 ]; then printf '%s\n' --enable-mbstring; fi ;;
  --needs)
    if [ "${ENABLE_ONIGURUMA:-0}" = 1 ]; then printf '%s\n' oniguruma; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
