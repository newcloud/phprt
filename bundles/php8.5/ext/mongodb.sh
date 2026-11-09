#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=mongodb switch=ENABLE_MONGODB version=MONGODB_VERSION modules=mongodb ini=20-mongodb.ini tools=autoconf,perlshim needs= optional= upstream=pecl:mongodb
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# PECL 包自带 libmongoc / libbson / libmongocrypt，不依赖外部驱动。
# zstd/snappy 默认 auto，会链接系统的 libzstd；关掉，保持树内自给。
build() {
  ext_pecl_build mongodb "$MONGODB_VERSION" "$MONGODB_URL" "$MONGODB_SHA256" \
    --with-mongodb-zstd=no --with-mongodb-snappy=no
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-mongodb.ini" <<INI
; 由 phprt 生成（mongodb $MONGODB_VERSION）
extension=mongodb.so
INI
}

recipe_main "$@"
