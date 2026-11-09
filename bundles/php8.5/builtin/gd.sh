#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=gd switch=ENABLE_GD modules=gd ini=- type=static,shared default=static
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_GD:-0}" = 1 ]; then
    printf '%s\n' --enable-gd
    if [ "${ENABLE_GD_JPEG:-1}" = 1 ]; then
      printf '%s\n' "--with-jpeg=$(dep_dir_of jpeg)"
    else
      printf '%s\n' --without-jpeg
    fi
    printf '%s\n' "--with-freetype=$(dep_dir_of freetype)"
    if [ "${ENABLE_GD_WEBP:-1}" = 1 ]; then
      printf '%s\n' "--with-webp=$(dep_dir_of webp)"
    else
      printf '%s\n' --without-webp
    fi
    if [ "${ENABLE_GD_AVIF:-1}" = 1 ]; then
      printf '%s\n' "--with-avif=$(dep_dir_of avif)"
    else
      printf '%s\n' --without-avif
    fi
  else
    printf '%s\n' --disable-gd
  fi
}

needs() {
  if [ "${ENABLE_GD:-0}" = 1 ]; then
    printf '%s\n' libpng freetype
    [ "${ENABLE_GD_JPEG:-1}" = 1 ] && printf '%s\n' jpeg
    [ "${ENABLE_GD_WEBP:-1}" = 1 ] && printf '%s\n' webp
    [ "${ENABLE_GD_AVIF:-1}" = 1 ] && printf '%s\n' aom avif
  fi
  return 0
}

recipe_main "$@"
