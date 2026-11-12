TARGET = dep(
    name = "openssl",
    version = "3.5.4",
    url = "https://github.com/openssl/openssl/releases/download/openssl-3.5.4/openssl-3.5.4.tar.gz",
    needs = [],
    configure = [
        "./Configure",
        "--prefix={prefix}",
        "--openssldir=/etc/pki/tls",
        "shared",
    ],
    make = ["make", "make install_sw"],
    artifacts = ["lib/libssl.so.3", "lib/libcrypto.so.3"],
    desc = "OpenSSL 库，供 PHP openssl 扩展链接",
)
