#!/usr/bin/env bash
# =============================================================================
#  内置扩展：libxml2 系列（dom / xml / simplexml / xmlreader / xmlwriter / soap）
#  phprt: builtin=libxml2 switch=ENABLE_LIBXML2
#  由 php.sh 调用，不单独构建：
#    --flags  输出 php configure 参数（一行一个）
#    --needs  输出依赖的库
#
#  PHP 7.4 的 ext/libxml 没有 config.m4，是强制启用的，也没有 --with-libxml 选项；
#  configure 通过 PKG_CONFIG_PATH 找树里的 libxml-2.0.pc。所以这里不给参数，
#  只保证依赖被构建、关闭时明确报错。
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    [ "${ENABLE_LIBXML2:-0}" = 1 ] ||
      die "PHP 7.4 的 libxml2 是强制依赖，--ext 必须包含 libxml2"
    # dom/xml/simplexml/xmlreader/xmlwriter 在 7.4 默认就编；soap 默认关闭
    printf '%s\n' --enable-soap
    ;;
  --needs)
    if [ "${ENABLE_LIBXML2:-0}" = 1 ]; then printf '%s\n' libxml2; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
