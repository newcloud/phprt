def target(ctx):
    self = ctx["self"]

    return dep(
        name = "c-ares",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-tests",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libcares.so*"],
        desc = "c-ares 异步 DNS 库，供 swoole 的 cares feature 使用",
    )


TARGET = target
