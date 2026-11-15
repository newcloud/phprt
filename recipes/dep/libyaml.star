def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libyaml",
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
        artifacts = ["lib/libyaml.so"],
        desc = "libyaml 库，供 PHP yaml 扩展链接",
    )


TARGET = target
