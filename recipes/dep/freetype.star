def target(ctx):
    self = ctx["self"]

    return dep(
        name = "freetype",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = ["zlib", "libpng"],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--with-zlib=yes",
            "--with-png=yes",
            "--with-harfbuzz=no",
            "--with-brotli=no",
            "--with-bzip2=no",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libfreetype.so"],
        desc = "FreeType 字体库，供 gd 的 FreeType 支持使用",
    )


TARGET = target
