#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=mongodb switch=ENABLE_MONGODB version=MONGODB_VERSION modules=mongodb ini=20-mongodb.ini tools=autoconf,perlshim needs= optional= upstream=pecl:mongodb type=static,shared default=shared
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# PECL 包自带 libmongoc / libbson / libmongocrypt，不依赖外部驱动。
# zstd/snappy 默认 auto，会链接系统的 libzstd；关掉，保持树内自给。
configure_args() {
  printf '%s\n' --enable-mongodb
  printf '%s\n' --with-mongodb-zstd=no
  printf '%s\n' --with-mongodb-snappy=no
}

build() {
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_pecl_build mongodb "$MONGODB_VERSION" "$MONGODB_URL" "$MONGODB_SHA256" "${_args[@]}"
}

stage_static() {
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_stage_static mongodb "$MONGODB_VERSION" "$MONGODB_URL" "$MONGODB_SHA256" "${_args[@]}"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-mongodb.ini" <<INI
; 由 phprt 生成（mongodb $MONGODB_VERSION）
extension=mongodb.so
INI
}

recipe_main "$@"
