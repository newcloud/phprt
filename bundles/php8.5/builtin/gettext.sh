#!/usr/bin/env bash
# 内置扩展：gettext（GNU gettext；glibc 自带，无需额外依赖库）
#  phprt: builtin=gettext switch=ENABLE_GETTEXT
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_GETTEXT:-0}" = 1 ]; then
      printf '%s\n' --with-gettext
    else
      printf '%s\n' --without-gettext
    fi ;;
  --needs) ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
