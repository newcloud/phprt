#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=gettext switch=ENABLE_GETTEXT modules=gettext ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  # glibc 自带 gettext，不需要额外依赖库
  if [ "${ENABLE_GETTEXT:-0}" = 1 ]; then
    printf '%s\n' --with-gettext
  else
    printf '%s\n' --without-gettext
  fi
}

needs() { :; }

recipe_main "$@"
