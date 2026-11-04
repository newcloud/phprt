#!/usr/bin/env bash
# =============================================================================
#  内置扩展：libxml2 系列（dom / xml / simplexml / xmlreader / xmlwriter / soap）
#  phprt: builtin=libxml2 switch=ENABLE_LIBXML2
#  由 php.sh 调用，不单独构建：
#    --flags  输出 php configure 参数（一行一个；开关关闭时输出负向参数）
#    --needs  输出依赖的库（空格分隔，随开关变化）
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_LIBXML2:-0}" = 1 ]; then
      printf '%s\n' --with-libxml --enable-dom --enable-xml --enable-simplexml \
        --enable-xmlreader --enable-xmlwriter --enable-soap
    else
      printf '%s\n' --without-libxml --disable-dom --disable-xml --disable-simplexml \
        --disable-xmlreader --disable-xmlwriter --disable-soap
    fi ;;
  --needs)
    if [ "${ENABLE_LIBXML2:-0}" = 1 ]; then printf '%s\n' libxml2; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
