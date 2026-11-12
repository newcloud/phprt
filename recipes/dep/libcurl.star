TARGET = dep(
    name = "libcurl",
    version = "8.11.1",
    url = "https://github.com/curl/curl/releases/download/curl-8_11_1/curl-8.11.1.tar.gz",
    needs = [],
    features = {
        "ssl": feature(
            needs = ["openssl"],
            configure = ["--with-openssl={dep:openssl}"],
        ),
        "nghttp2": feature(
            needs = ["nghttp2"],
            configure = ["--with-nghttp2={dep:nghttp2}"],
        ),
    },
    configure = [
        "./configure",
        "--prefix={prefix}",
        "--disable-ldap",
        "--without-libpsl",
    ],
    make = ["make", "make install"],
    artifacts = ["lib/libcurl.so.4"],
    desc = "libcurl 库，供 PHP curl 扩展链接",
)
