#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=imagemagick version=IMAGEMAGICK_VERSION needs=zlib,libpng,jpeg,freetype,webp optional= tools= upstream=none
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  need_cmds gcc g++ make tar pkg-config

  local D SIG TAR SRC
  D="$(dep_dir_of imagemagick)"
  SIG="imagemagick=$IMAGEMAGICK_VERSION;png=$LIBPNG_VERSION;jpeg=$JPEG_VERSION;ft=$FREETYPE_VERSION;webp=$WEBP_VERSION;zlib=$ZLIB_VERSION"

  if [ "$FORCE" != 1 ] && stamp_ok imagemagick "$SIG" && [ -f "$D/lib/libMagickWand-7.Q16HDRI.so" ]; then
    ok "ImageMagick 已安装，跳过"; return 0
  fi

  ensure_needs imagemagick
  dep_env imagemagick
  ensure_clean_target imagemagick "$D"

  TAR="$(basename "$IMAGEMAGICK_URL")"
  fetch "$IMAGEMAGICK_URL" "$DOWNLOADS/$TAR" "${IMAGEMAGICK_SHA256:-}"

  SRC="$SRCDIR/ImageMagick-$IMAGEMAGICK_VERSION"
  if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
    log "复用源码 $SRC"
    ( cd "$SRC" && make distclean >/dev/null 2>&1 || true )
  else
    rm -rf "$SRC"
    log "解压 $TAR"
    extract "$DOWNLOADS/$TAR" "$SRCDIR"
  fi

  cd "$SRC"
  # 只启用树内已有的图片格式；其余全部关掉，避免链接系统库
  log "configure + make -j$JOBS"
  ./configure --prefix="$D" --disable-static --enable-shared \
    --without-magick-plus-plus --disable-openmp --without-x --without-perl \
    --without-utilities --without-gslib \
    --without-bzlib --without-zip --without-zstd --without-lzma --without-djvu \
    --without-fontconfig --without-heic --without-jxl --without-lcms \
    --without-openjp2 --without-lqr --without-openexr --without-pango \
    --without-tiff --without-xml \
    > "$LOGS/imagemagick-configure.log" 2>&1 \
    || { tail -40 "$LOGS/imagemagick-configure.log"; die "ImageMagick configure 失败"; }
  make -j"$JOBS" > "$LOGS/imagemagick-make.log" 2>&1 \
    || { tail -40 "$LOGS/imagemagick-make.log"; die "ImageMagick make 失败"; }
  make install >> "$LOGS/imagemagick-make.log" 2>&1

  stamp_write imagemagick "$SIG"
  ok "ImageMagick $IMAGEMAGICK_VERSION -> $D"
}

recipe_main "$@"
