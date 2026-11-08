#!/usr/bin/env bash
# 内置扩展：xsl（XSLT 转换；Magento 硬性要求，依赖 libxslt）
#  phprt: builtin=xsl switch=ENABLE_XSL
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_XSL:-0}" = 1 ]; then
      printf '%s\n' "--with-xsl=$(dep_dir_of libxslt)"
    else
      printf '%s\n' --without-xsl
    fi ;;
  --needs)
    if [ "${ENABLE_XSL:-0}" = 1 ]; then
      printf '%s\n' libxslt
    fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
