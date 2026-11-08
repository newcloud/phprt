#!/usr/bin/env bash
# 内置扩展：sodium（现代加密；PHP 7.2 起自带，需要 libsodium）
#  phprt: builtin=sodium switch=ENABLE_SODIUM
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_SODIUM:-0}" = 1 ]; then
      printf '%s\n' "--with-sodium=$(dep_dir_of libsodium)"
    else
      printf '%s\n' --without-sodium
    fi ;;
  --needs)
    if [ "${ENABLE_SODIUM:-0}" = 1 ]; then
      printf '%s\n' libsodium
    fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
