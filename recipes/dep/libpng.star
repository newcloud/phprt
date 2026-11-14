def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libpng",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = ["zlib"],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-static",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libpng16.so"],
        desc = "libpng 库，PHP 8.5 gd 的 PNG 支持必需",
    )


TARGET = target
