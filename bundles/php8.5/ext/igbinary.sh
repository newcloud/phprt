#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=igbinary switch=ENABLE_IGBINARY version=IGBINARY_VERSION modules=igbinary ini=20-igbinary.ini tools=autoconf,perlshim needs= optional= upstream=pecl:igbinary
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

# PHP 8.4 起 ext/standard/php_smart_string.h 被移除；igbinary 3.2.16 只 include
# 不用它（全源码无 smart_str/smart_string 调用），删掉即可编译
ext_prepare() {
  local hdr="src/php7/php_igbinary.h"
  [ -f "$hdr" ] || die "igbinary 源码里没有 $hdr"
  grep -q 'PHPRT_PHP84_COMPAT' "$hdr" && return 0
  log "PHP 8.4+ 兼容补丁（删除未使用的 php_smart_string.h include）"
  sed -i '\|#include "ext/standard/php_smart_string.h"|d' "$hdr"
  echo '/* PHPRT_PHP84_COMPAT: php_smart_string.h removed in PHP 8.4; include was unused */' \
    >> "$hdr"
}

build() {
  ext_pecl_build igbinary "$IGBINARY_VERSION" "$IGBINARY_URL" "$IGBINARY_SHA256" \
    --enable-igbinary
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-igbinary.ini" <<INI
; 由 phprt 生成（igbinary $IGBINARY_VERSION）
extension=igbinary.so
INI
}

recipe_main "$@"
