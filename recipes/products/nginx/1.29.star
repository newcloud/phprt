# nginx 1.29：PCRE2、OpenSSL 和 zlib 用 phprt 编译好的依赖库，
# 组件模块折叠进 nginx 的 ./configure。

LINES = {
    "1.29": {
        "openssl": ["3.0", "3.5", "4.0"],
        "pcre2": ["10.49"],
        "zlib": ["1.3"],
    },
}


TOOLCHAIN = toolchain(
    gcc = [toolchain_range(min = "8")],
)

TARGET = product(
    name = "nginx",
    toolchain = TOOLCHAIN,
    series = "1.29",
    needs = ["pcre2", "openssl", "zlib"],
    configure = [
        "./configure",
        "--prefix={tree}",
        "--sbin-path={tree}/sbin/nginx",
        "--conf-path={tree}/etc/nginx.conf",
        "--pid-path={tree}/var/nginx.pid",
        "--lock-path={tree}/var/nginx.lock",
        "--http-log-path={tree}/var/log/access.log",
        "--error-log-path={tree}/var/log/error.log",
        "--with-cc-opt=-I{dep:openssl}/include -I{dep:pcre2}/include -I{dep:zlib}/include",
        "--with-ld-opt=-L{dep:openssl}/lib -L{dep:pcre2}/lib -L{dep:zlib}/lib",
        "--with-http_ssl_module",
        "--with-http_v2_module",
        "--with-http_stub_status_module",
    ],
    make = ["make", "make install"],
    artifacts = ["sbin/nginx"],
    profile = PROFILE,
    desc = "nginx 1.29 本体",
)
