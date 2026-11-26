def target(ctx):
    self = ctx["self"]

    # 只装静态库。nginx / PHP 自己生成的 -lcrypt 会先命中 libcrypt.a，
    # 产物不再带 DT_NEEDED libcrypt.so.1，换发行版也能直接运行。
    # obsolete API 只影响给旧二进制提供的 crypt@GLIBC_2.2.5 兼容符号，
    # 新编译的目标不需要。
    return dep(
        name = "libxcrypt",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        # configure 用 build-aux/scripts/expand-selected-hashes（Perl）展开哈希列表；
        # Debian / Ubuntu 的 perl-base 缺 open.pm，必须要求完整 perl 包。
        tools = [tool("perl", min = "5.20", feature_checks = ["open"])],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-shared",
            "--enable-static",
            "--enable-obsolete-api=no",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libcrypt.a"],
        desc = "libxcrypt 静态库，供 nginx / PHP 的 crypt() 静态链接",
    )


TARGET = target
