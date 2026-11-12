TARGET = dep(
    name = "nghttp2",
    version = "1.64.0",
    url = "https://github.com/nghttp2/nghttp2/releases/download/v1.64.0/nghttp2-1.64.0.tar.gz",
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
