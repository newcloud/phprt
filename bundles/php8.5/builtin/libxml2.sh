#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=libxml2 switch=ENABLE_LIBXML2 modules=dom,xml,simplexml,xmlreader,xmlwriter,soap ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_LIBXML2:-0}" = 1 ]; then
    printf '%s\n' --with-libxml --enable-dom --enable-xml --enable-simplexml \
      --enable-xmlreader --enable-xmlwriter --enable-soap
  else
    printf '%s\n' --without-libxml --disable-dom --disable-xml --disable-simplexml \
      --disable-xmlreader --disable-xmlwriter --disable-soap
  fi
}

needs() {
  [ "${ENABLE_LIBXML2:-0}" = 1 ] && printf '%s\n' libxml2
  return 0
}

recipe_main "$@"
