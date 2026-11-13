def target(ctx):
    self = ctx["self"]
    line = self["line"]

    if line == "1.1.1":
        configure = [
            "./config",
            "--prefix={prefix}",
            "--openssldir=/etc/pki/tls",
            "--libdir=lib",
            "shared",
            "no-tests",
        ]
        artifacts = ["lib/libssl.so.1.1", "lib/libcrypto.so.1.1"]
    else:
        major = line.split(".")[0]
        configure = [
            "./Configure",
            "--prefix={prefix}",
            "--openssldir=/etc/pki/tls",
            "--libdir=lib",
            "shared",
            "no-tests",
        ]
        artifacts = ["lib/libssl.so." + major, "lib/libcrypto.so." + major]

    return dep(
        name = "openssl",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        configure = configure,
        make = ["make", "make install_sw"],
        artifacts = artifacts,
        desc = "OpenSSL 库，供 PHP openssl 扩展链接",
    )


TARGET = target
