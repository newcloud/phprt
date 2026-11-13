def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libcurl",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        features = {
            "ssl": feature(
                needs = ["openssl"],
                configure = ["--with-openssl={dep:openssl}"],
                disable = ["--without-ssl"],
            ),
            "nghttp2": feature(
                needs = ["nghttp2"],
                configure = ["--with-nghttp2={dep:nghttp2}"],
                disable = ["--without-nghttp2"],
            ),
        },
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-ldap",
            "--disable-ldaps",
            "--disable-manual",
            "--without-libpsl",
            "--without-zstd",
            "--without-brotli",
            "--without-libidn2",
            "--without-libssh2",
            "--without-nghttp3",
            "--without-ngtcp2",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libcurl.so.4"],
        desc = "libcurl 库，供 PHP curl 扩展链接",
    )


TARGET = target
