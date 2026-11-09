#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=openssl switch=ENABLE_OPENSSL modules=openssl ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_OPENSSL:-0}" = 1 ]; then
    printf '%s\n' "--with-openssl=$(dep_dir_of openssl)"
    # PHP 8.5+：用已编好的 OpenSSL 3.2+ 提供 argon2i/argon2id
    printf '%s\n' --with-openssl-argon2
  else
    printf '%s\n' --without-openssl
  fi
}

needs() {
  [ "${ENABLE_OPENSSL:-0}" = 1 ] && printf '%s\n' openssl
}

recipe_main "$@"
