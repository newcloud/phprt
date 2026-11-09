#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=ssh2 switch=ENABLE_SSH2 version=SSH2_VERSION modules=ssh2 ini=20-ssh2.ini tools=autoconf,perlshim needs=libssh2 optional= upstream=pecl:ssh2
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  dep_env ssh2
  ext_pecl_build ssh2 "$SSH2_VERSION" "$SSH2_URL" "$SSH2_SHA256" \
    --with-ssh2="$(dep_dir_of libssh2)"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-ssh2.ini" <<INI
; 由 phprt 生成（ssh2 $SSH2_VERSION / libssh2 $LIBSSH2_VERSION）
extension=ssh2.so
INI
}

recipe_main "$@"
