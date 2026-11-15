def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libsodium",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-static",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libsodium.so"],
        desc = "libsodium 库，供 PHP sodium 扩展链接",
    )


TARGET = target
