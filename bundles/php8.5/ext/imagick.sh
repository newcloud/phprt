#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=imagick switch=ENABLE_IMAGICK version=IMAGICK_VERSION modules=imagick ini=20-imagick.ini tools=autoconf,perlshim needs=imagemagick optional= upstream=pecl:imagick
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  # imagick 的 config.m4 在 GCC 下无条件加 -lgomp（系统 OpenMP）；
  # 预先把这个探测结果置 no，避免 imagick.so 借用安装根外的 libgomp。
  export ac_cv_func_omp_pause_resource_all=no
  dep_env imagick
  ext_pecl_build imagick "$IMAGICK_VERSION" "$IMAGICK_URL" "$IMAGICK_SHA256" \
    --with-imagick="$(dep_dir_of imagemagick)"
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-imagick.ini" <<INI
; 由 phprt 生成（imagick $IMAGICK_VERSION / ImageMagick $IMAGEMAGICK_VERSION）
extension=imagick.so
INI
}

recipe_main "$@"
