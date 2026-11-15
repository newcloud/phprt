def target(ctx):
    self = ctx["self"]

    return dep(
        name = "oniguruma",
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
        artifacts = ["lib/libonig.so"],
        desc = "oniguruma 正则库，供 PHP mbstring 的 mbregex 使用",
    )


TARGET = target
