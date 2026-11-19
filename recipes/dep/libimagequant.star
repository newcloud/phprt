def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libimagequant",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        configure = [
            "./configure",
            "--prefix={prefix}",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libimagequant.so"],
        desc = "libimagequant 库，供 gd 的调色板量化支持使用",
    )


TARGET = target
