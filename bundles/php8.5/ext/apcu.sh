#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=apcu switch=ENABLE_APCU version=APCU_VERSION modules=apcu ini=25-apcu.ini tools=autoconf,perlshim needs= optional= upstream=pecl:apcu
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  ext_pecl_build apcu "$APCU_VERSION" "$APCU_URL" "$APCU_SHA256" --enable-apcu
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/25-apcu.ini" <<INI
; 由 phprt 生成（apcu $APCU_VERSION）
extension=apcu.so
apc.enabled=1
apc.enable_cli=1
apc.shm_size=32M
apc.ttl=0
INI
}

recipe_main "$@"
