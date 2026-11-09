#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=yaml switch=ENABLE_YAML version=YAML_EXT_VERSION modules=yaml ini=26-yaml.ini tools=autoconf,perlshim needs=libyaml optional= upstream=pecl:yaml type=static,shared default=shared
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

configure_args() {
  printf '%s\n' --with-yaml="$(dep_dir_of libyaml)"
}

build() {
  dep_env yaml
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_pecl_build yaml "$YAML_EXT_VERSION" "$YAML_EXT_URL" "$YAML_EXT_SHA256" "${_args[@]}"
}

stage_static() {
  dep_env yaml
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_stage_static yaml "$YAML_EXT_VERSION" "$YAML_EXT_URL" "$YAML_EXT_SHA256" "${_args[@]}"
}

static_ldflags() {
  printf '%s\n' -lyaml
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/26-yaml.ini" <<INI
; 由 phprt 生成（yaml $YAML_EXT_VERSION / libyaml $LIBYAML_VERSION）
extension=yaml.so
INI
}

recipe_main "$@"
