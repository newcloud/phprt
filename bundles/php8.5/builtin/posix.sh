#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=posix switch=ENABLE_POSIX modules=posix ini=- type=static,shared default=static category=Runtime note=posix-default-on
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# PHP 的 ext/posix 默认就是启用的，不选时必须显式 --disable-posix。
flags() {
  if [ "${ENABLE_POSIX:-0}" = 1 ]; then
    printf '%s\n' --enable-posix
  else
    printf '%s\n' --disable-posix
  fi
}

needs() { :; }

recipe_main "$@"
