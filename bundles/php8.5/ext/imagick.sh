#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=imagick switch=ENABLE_IMAGICK version=IMAGICK_VERSION modules=imagick ini=20-imagick.ini tools=autoconf,perlshim needs=imagemagick optional= upstream=pecl:imagick type=static,shared default=shared
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

configure_args() {
  printf '%s\n' ac_cv_func_omp_pause_resource_all=no
  printf '%s\n' --with-imagick="$(dep_dir_of imagemagick)"
}

build() {
  dep_env imagick
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_pecl_build imagick "$IMAGICK_VERSION" "$IMAGICK_URL" "$IMAGICK_SHA256" "${_args[@]}"
}

stage_static() {
  dep_env imagick
  local -a _args=()
  while IFS= read -r a; do [ -n "$a" ] && _args+=("$a"); done < <(configure_args)
  ext_stage_static imagick "$IMAGICK_VERSION" "$IMAGICK_URL" "$IMAGICK_SHA256" "${_args[@]}"
}

static_ldflags() {
  printf '%s\n' -lMagickWand-7.Q16HDRI -lMagickCore-7.Q16HDRI
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-imagick.ini" <<INI
; 由 phprt 生成（imagick $IMAGICK_VERSION / ImageMagick $IMAGEMAGICK_VERSION）
extension=imagick.so
INI
}

recipe_main "$@"
