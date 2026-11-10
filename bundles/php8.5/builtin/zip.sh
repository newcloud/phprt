#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=zip switch=ENABLE_LIBZIP modules=zip ini=- type=static,shared default=static category=Compression
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# libzip 的 bzip2 后端是可选的；默认关，保持和以前一样的构建结果。
opts() {
  printf '%s\t%s\t%s\n' \
    bzip2 off 'bzip2 压缩'
}

flags() {
  # zip 默认不编译，不需要负向参数
  if [ "${ENABLE_LIBZIP:-0}" = 1 ]; then
    printf '%s\n' "--with-zip=$(dep_dir_of libzip)"
  else
    printf '%s\n' --without-zip
  fi
}

needs() {
  if [ "${ENABLE_LIBZIP:-0}" = 1 ]; then
    printf '%s\n' libzip
    [ "${ENABLE_LIBZIP_BZIP2:-0}" = 1 ] && printf '%s\n' bzip2
  fi
  return 0
}

recipe_main "$@"
