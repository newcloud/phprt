#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=pcntl switch=ENABLE_PCNTL modules=pcntl ini=- type=static,shared default=static category=Runtime
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_PCNTL:-0}" = 1 ]; then
    printf '%s\n' --enable-pcntl
  else
    printf '%s\n' --disable-pcntl
  fi
}

needs() { :; }

recipe_main "$@"
