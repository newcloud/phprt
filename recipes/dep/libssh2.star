def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libssh2",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = ["openssl", "zlib"],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-static",
            "--with-crypto=openssl",
            "--with-libz",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libssh2.so"],
        desc = "libssh2 库，供 PHP ssh2 扩展链接",
    )


TARGET = target
