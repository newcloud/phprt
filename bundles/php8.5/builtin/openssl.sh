#!/usr/bin/env bash
# =============================================================================
#  内置扩展：OpenSSL（TLS）
#  phprt: builtin=openssl switch=ENABLE_OPENSSL
#  由 php.sh 调用，不单独构建：
#    --flags  输出 php configure 参数（一行一个；开关关闭时输出负向参数）
#    --needs  输出依赖的库（空格分隔，随开关变化）
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_OPENSSL:-0}" = 1 ]; then
      printf '%s\n' "--with-openssl=$(dep_dir_of openssl)"
      # PHP 8.5+：用已编好的 OpenSSL 3.2+ 提供 argon2i/argon2id，
      # 不引入额外的 libargon2（PHP 8.4 及以前没有这个参数）
      printf '%s\n' --with-openssl-argon2
    else
      printf '%s\n' "--without-openssl"
    fi ;;
  --needs)
    if [ "${ENABLE_OPENSSL:-0}" = 1 ]; then printf '%s\n' openssl; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
