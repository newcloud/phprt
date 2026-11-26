# nginx 1.29：PCRE2、OpenSSL、zlib 用 phprt 编译好的依赖库，
# 组件模块折叠进 nginx 的 ./configure。
#
# --with-cc-opt / --with-ld-opt 是单值选项（同一个选项出现两次，后一个覆盖前一个），
# 所以用 configure_options 写：基础值和 feature 追加的值按顺序合并成一条参数。
# Feature `xcrypt`（bundle 默认启用）把 libxcrypt 只编成静态库，nginx 的 -lcrypt
# 命中 libcrypt.a，产物不再依赖宿主 libcrypt.so.1，Debian 编的包也能在 Fedora 上直接跑。
# 关掉 feature 时合并结果里没有 libxcrypt 的 -L，不会因为 deps/ 里留着上次的
# libcrypt.a 又悄悄静态链进去。

LINES = {
    "1.29": {
        "openssl": ["3.0", "3.5", "4.0"],
        "pcre2": ["10.49"],
        "zlib": ["1.3"],
        "libxcrypt": ["4.5"],
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
    features = {
        "xcrypt": feature(
            needs = ["libxcrypt"],
            configure_options = {
                "--with-ld-opt": [
                    "-L{dep:libxcrypt}/lib",
                ],
            },
        ),
    },
    configure = [
        "./configure",
        "--prefix={tree}",
        "--sbin-path={tree}/sbin/nginx",
        "--conf-path={tree}/etc/nginx.conf",
        "--pid-path={tree}/var/nginx.pid",
        "--lock-path={tree}/var/nginx.lock",
        "--http-log-path={tree}/var/log/access.log",
        "--error-log-path={tree}/var/log/error.log",
        "--with-http_ssl_module",
        "--with-http_v2_module",
        "--with-http_stub_status_module",
    ],
    configure_options = {
        "--with-cc-opt": [
            "-I{dep:openssl}/include",
            "-I{dep:pcre2}/include",
            "-I{dep:zlib}/include",
        ],
        "--with-ld-opt": [
            "-L{dep:openssl}/lib",
            "-L{dep:pcre2}/lib",
            "-L{dep:zlib}/lib",
        ],
    },
    make = ["make", "make install"],
    artifacts = ["sbin/nginx"],
    profile = PROFILE,
    desc = "nginx 1.29 本体",
)
