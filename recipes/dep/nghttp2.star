def target(ctx):
    self = ctx["self"]

    return dep(
        name = "nghttp2",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--enable-lib-only",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libnghttp2.so.14"],
        desc = "HTTP/2 库，供 libcurl 的 nghttp2 子功能使用",
    )


TARGET = target
