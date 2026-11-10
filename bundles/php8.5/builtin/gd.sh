#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=gd switch=ENABLE_GD modules=gd ini=- type=static,shared default=static category=Image
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# gd 的子选项。驱动只认 key/state/label，不认具体名字：
#   on    = 默认启用
#   off   = 默认关闭
#   fixed = 始终启用，界面只展示、不允许关闭
# key 会映射成 ${switch}_${KEY}，例如 jpeg -> ENABLE_GD_JPEG。
opts() {
  printf '%s\t%s\t%s\n' \
    jpeg     on    'JPEG 照片' \
    png      fixed 'PNG（PHP 8.5 gd 必需）' \
    webp     on    'WebP' \
    avif     off   'AVIF（可选，默认关闭）' \
    freetype on    'FreeType 字体'
}

flags() {
  if [ "${ENABLE_GD:-0}" = 1 ]; then
    printf '%s\n' --enable-gd
    if [ "${ENABLE_GD_JPEG:-1}" = 1 ]; then
      printf '%s\n' "--with-jpeg=$(dep_dir_of jpeg)"
    else
      printf '%s\n' --without-jpeg
    fi
    if [ "${ENABLE_GD_FREETYPE:-1}" = 1 ]; then
      printf '%s\n' "--with-freetype=$(dep_dir_of freetype)"
    else
      printf '%s\n' --without-freetype
    fi
    if [ "${ENABLE_GD_WEBP:-1}" = 1 ]; then
      printf '%s\n' "--with-webp=$(dep_dir_of webp)"
    else
      printf '%s\n' --without-webp
    fi
    if [ "${ENABLE_GD_AVIF:-0}" = 1 ]; then
      printf '%s\n' "--with-avif=$(dep_dir_of avif)"
    else
      printf '%s\n' --without-avif
    fi
  else
    printf '%s\n' --disable-gd
  fi
}

needs() {
  if [ "${ENABLE_GD:-0}" = 1 ]; then
    # PHP 8.5 的 gd 强制要求 libpng，png 不可关闭。
    printf '%s\n' libpng
    [ "${ENABLE_GD_JPEG:-1}" = 1 ] && printf '%s\n' jpeg
    [ "${ENABLE_GD_FREETYPE:-1}" = 1 ] && printf '%s\n' freetype
    [ "${ENABLE_GD_WEBP:-1}" = 1 ] && printf '%s\n' webp
    [ "${ENABLE_GD_AVIF:-0}" = 1 ] && printf '%s\n' aom avif
  fi
  return 0
}

recipe_main "$@"
