def target(ctx):
    self = ctx["self"]

    return dep(
        name = "librdkafka",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = ["openssl", "zlib"],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--enable-ssl",
            "--enable-zlib",
            "--disable-gssapi",
            "--disable-sasl",
            "--disable-zstd",
            "--disable-curl",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/librdkafka.so"],
        desc = "librdkafka 库，供 PHP rdkafka 扩展链接",
    )


TARGET = target
