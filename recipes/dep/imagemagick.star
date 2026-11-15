def target(ctx):
    self = ctx["self"]

    return dep(
        name = "imagemagick",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        # 图片格式只打开树内已经编好的库，其余一律显式关闭，避免探测到系统库。
        needs = ["zlib", "libpng", "jpeg", "freetype", "webp"],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-static",
            "--enable-shared",
            "--without-magick-plus-plus",
            "--disable-openmp",
            "--without-x",
            "--without-perl",
            "--without-utilities",
            "--without-gslib",
            "--without-bzlib",
            "--without-zip",
            "--without-zstd",
            "--without-lzma",
            "--without-djvu",
            "--without-fontconfig",
            "--without-heic",
            "--without-jxl",
            "--without-lcms",
            "--without-openjp2",
            "--without-lqr",
            "--without-openexr",
            "--without-pango",
            "--without-tiff",
            "--without-xml",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libMagickWand-7.Q16HDRI.so"],
        desc = "ImageMagick 库，供 PHP imagick 扩展链接；只启用 PNG/JPEG/WebP/FreeType",
    )


TARGET = target
