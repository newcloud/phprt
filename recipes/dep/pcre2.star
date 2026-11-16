def target(ctx):
    self = ctx["self"]

    return dep(
        name = "pcre2",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--enable-pcre2-8",
            "--disable-pcre2-16",
            "--disable-pcre2-32",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libpcre2-8.so"],
        desc = "PCRE2 正则库，供 nginx 的 location 正则使用",
    )


TARGET = target
