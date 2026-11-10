#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=__NAME__ switch=ENABLE___UPPER__ version=__VERSION_VAR__ modules=__NAME__ ini=20-__NAME__.ini tools=autoconf,perlshim needs= optional= upstream=none type=static,shared default=shared category=__CATEGORY__ note=
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  printf '%s\n' --enable-__NAME__
}

build() {
  local VAR=__VERSION_VAR__ VER="${!VAR}" U="${VAR%_VERSION}_URL" S="${VAR%_VERSION}_SHA256"
  ext_pecl_build __NAME__ "$VER" "${!U}" "${!S:-}" $(flags)
}

enable() {
  local VAR=__VERSION_VAR__ VER="${!VAR}"
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-__NAME__.ini" <<INI
; 由 phprt 生成（__NAME__ $VER）
extension=__NAME__.so
INI
}

recipe_main "$@"
