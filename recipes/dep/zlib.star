def target(ctx):
    self = ctx["self"]

    return dep(
        name = "zlib",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        configure = [
            "./configure",
            "--prefix={prefix}",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libz.so"],
        desc = "zlib 压缩库，供 libpng/freetype 使用",
    )


TARGET = target
