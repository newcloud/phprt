#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=curl switch=ENABLE_LIBCURL modules=curl ini=- type=static,shared default=static category=Network
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# SFTP/SCP 由 libcurl 的 libssh2 后端提供；默认关，避免平白多编一个 libssh2。
opts() {
  printf '%s\t%s\t%s\n' \
    sftp off 'SFTP/SCP（复用 libssh2）'
}

flags() {
  if [ "${ENABLE_LIBCURL:-0}" = 1 ]; then
    printf '%s\n' "--with-curl=$(dep_dir_of curl)"
  else
    printf '%s\n' --without-curl
  fi
}

needs() {
  if [ "${ENABLE_LIBCURL:-0}" = 1 ]; then
    printf '%s\n' curl
    [ "${ENABLE_LIBCURL_SFTP:-0}" = 1 ] && printf '%s\n' libssh2
  fi
  return 0
}

recipe_main "$@"
