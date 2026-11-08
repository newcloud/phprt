#!/usr/bin/env bash
# 内置扩展：bz2（bzip2 压缩；依赖 bzip2 库）
#  phprt: builtin=bz2 switch=ENABLE_BZ2
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_BZ2:-0}" = 1 ]; then
      printf '%s\n' "--with-bz2=$(dep_dir_of bzip2)"
    else
      printf '%s\n' --without-bz2
    fi ;;
  --needs)
    if [ "${ENABLE_BZ2:-0}" = 1 ]; then
      printf '%s\n' bzip2
    fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
