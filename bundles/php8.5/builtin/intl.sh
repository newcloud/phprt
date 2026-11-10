#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=intl switch=ENABLE_INTL modules=intl ini=- type=static,shared default=static category=Internationalization
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  # PHP 8.5+ 只用 pkg-config 定位 ICU（--with-icu-dir 已被上游移除）
  if [ "${ENABLE_INTL:-0}" = 1 ]; then
    printf '%s\n' --enable-intl
  else
    printf '%s\n' --disable-intl
  fi
}

needs() {
  [ "${ENABLE_INTL:-0}" = 1 ] && printf '%s\n' icu
  return 0
}

recipe_main "$@"
