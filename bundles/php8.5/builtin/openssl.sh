#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=openssl switch=ENABLE_OPENSSL modules=openssl ini=- type=static default=static category=Cryptography
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# 默认加载 legacy provider：老服务端和老算法（MD4/RC4/DES 等）走这条路。
opts() {
  printf '%s\t%s\t%s\n' \
    legacy_provider off 'OpenSSL legacy provider（老算法/老服务端）'
}

flags() {
  if [ "${ENABLE_OPENSSL:-0}" = 1 ]; then
    printf '%s\n' "--with-openssl=$(dep_dir_of openssl)"
    # PHP 8.5+：用已编好的 OpenSSL 3.2+ 提供 argon2i/argon2id
    printf '%s\n' --with-openssl-argon2
    [ "${ENABLE_OPENSSL_LEGACY_PROVIDER:-0}" = 1 ] && printf '%s\n' --with-openssl-legacy-provider
  else
    printf '%s\n' --without-openssl
  fi
}

needs() {
  [ "${ENABLE_OPENSSL:-0}" = 1 ] && printf '%s\n' openssl
}

recipe_main "$@"
