#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=yaml switch=ENABLE_YAML version=YAML_EXT_VERSION modules=yaml ini=26-yaml.ini tools=autoconf,perlshim needs=libyaml optional= upstream=pecl:yaml
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  dep_env yaml
  ext_pecl_build yaml "$YAML_EXT_VERSION" "$YAML_EXT_URL" "$YAML_EXT_SHA256" \
    --with-yaml="$(dep_dir_of libyaml)"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/26-yaml.ini" <<INI
; 由 phprt 生成（yaml $YAML_EXT_VERSION / libyaml $LIBYAML_VERSION）
extension=yaml.so
INI
}

recipe_main "$@"
