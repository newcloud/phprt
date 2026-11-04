#!/usr/bin/env bash
# =============================================================================
#  内置扩展：gd（PNG / JPEG / FreeType，可选 WebP / AVIF）
#  phprt: builtin=gd switch=ENABLE_GD
#  由 php.sh 调用，不单独构建：
#    --flags  输出 php configure 参数（一行一个；开关关闭时输出负向参数）
#    --needs  输出依赖的库（空格分隔，随开关变化）
# =============================================================================
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_GD:-0}" = 1 ]; then
      printf '%s\n' --enable-gd \
        "--with-jpeg=$(dep_dir_of jpeg)" "--with-freetype=$(dep_dir_of freetype)"
      if [ "${ENABLE_WEBP:-0}" = 1 ]; then printf '%s\n' "--with-webp=$(dep_dir_of webp)"; fi
      if [ "${ENABLE_AVIF:-0}" = 1 ]; then printf '%s\n' "--with-avif=$(dep_dir_of avif)"; fi
    else
      printf '%s\n' --disable-gd
    fi ;;
  --needs)
    if [ "${ENABLE_GD:-0}" = 1 ]; then
      printf '%s\n' libpng jpeg freetype
      if [ "${ENABLE_WEBP:-0}" = 1 ]; then printf '%s\n' webp; fi
      if [ "${ENABLE_AVIF:-0}" = 1 ]; then printf '%s\n' aom avif; fi
    fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
