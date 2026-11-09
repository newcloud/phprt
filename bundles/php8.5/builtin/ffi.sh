#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=ffi switch=ENABLE_FFI modules=ffi ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  # FFI 默认关闭，不需要负向参数
  if [ "${ENABLE_FFI:-0}" = 1 ]; then
    printf '%s\n' "--with-ffi=$(dep_dir_of libffi)"
  fi
}

needs() {
  [ "${ENABLE_FFI:-0}" = 1 ] && printf '%s\n' libffi
  return 0
}

recipe_main "$@"
