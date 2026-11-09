#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=xsl switch=ENABLE_XSL modules=xsl ini=- type=static,shared default=static
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_XSL:-0}" = 1 ]; then
    printf '%s\n' "--with-xsl=$(dep_dir_of libxslt)"
  else
    printf '%s\n' --without-xsl
  fi
}

needs() {
  [ "${ENABLE_XSL:-0}" = 1 ] && printf '%s\n' libxslt
  return 0
}

recipe_main "$@"
