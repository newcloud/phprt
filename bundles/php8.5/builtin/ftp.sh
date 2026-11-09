#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=ftp switch=ENABLE_FTP modules=ftp ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  # 与 openssl 同编时 FTP-SSL 隐式启用
  if [ "${ENABLE_FTP:-0}" = 1 ]; then
    printf '%s\n' --enable-ftp
  else
    printf '%s\n' --disable-ftp
  fi
}

needs() { :; }

recipe_main "$@"
