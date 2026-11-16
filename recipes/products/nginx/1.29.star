# nginx 1.29：PCRE2 和 OpenSSL 用 phprt 编译好的依赖库，
# 组件模块折叠进 nginx 的 ./configure。

LINES = {
    "1.29": {
        "openssl": ["3.0", "3.5", "4.0"],
        "pcre2": ["10.49"],
    },
}


def nginx_profile():
    return product_profile(
        modes = {
            "builtin": mode(
                strategy = "fold",
                configure = ["--add-module={component_source}"],
            ),
            "dynamic": mode(
                strategy = "fold",
                configure = ["--add-dynamic-module={component_source}"],
                artifacts = ["modules/*.so"],
            ),
        },
        defaults = {"bundled": "builtin", "external": "dynamic"},
        runtime = runtime(
            lib_dir = "lib/phprt",
            rpath = {
                "sbin/nginx": "$ORIGIN/../lib/phprt",
                "modules/*.so": "$ORIGIN/../lib/phprt",
            },
        ),
        verify = ["{tree}/sbin/nginx", "-V"],
        entry = "",
        audit = "none",
    )


TARGET = product(
    name = "nginx",
    series = "1.29",
    needs = ["pcre2", "openssl"],
    configure = [
        "./configure",
        "--prefix={tree}",
        "--sbin-path={tree}/sbin/nginx",
        "--conf-path={tree}/etc/nginx.conf",
        "--pid-path={tree}/var/nginx.pid",
        "--lock-path={tree}/var/nginx.lock",
        "--http-log-path={tree}/var/log/access.log",
        "--error-log-path={tree}/var/log/error.log",
        "--with-cc-opt=-I{dep:openssl}/include -I{dep:pcre2}/include",
        "--with-ld-opt=-L{dep:openssl}/lib -L{dep:pcre2}/lib",
        "--with-http_ssl_module",
        "--with-http_v2_module",
        "--with-http_stub_status_module",
    ],
    make = ["make", "make install"],
    artifacts = ["sbin/nginx"],
    profile = nginx_profile(),
    desc = "nginx 1.29 本体",
)
