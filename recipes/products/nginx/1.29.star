# nginx 1.29：PCRE2、OpenSSL、zlib 用 phprt 编译好的依赖库，
# 组件模块折叠进 nginx 的 ./configure。
#
# feature `xcrypt`（bundle 默认启用）把 libxcrypt 只编成静态库，nginx 的
# -lcrypt 命中 libcrypt.a，产物不再依赖宿主 libcrypt.so.1，Debian 编的包
# 也能在 Fedora 上直接跑。
#
# last-wins：nginx 的 --with-ld-opt 只保留最后一次出现的值，所以 feature 打开时
# 用一份包含 libxcrypt 的完整 -L 覆盖基础参数；关掉时基础参数里没有 libxcrypt，
# 不会因为 deps/ 里留着上次的 libcrypt.a 又悄悄静态链进去。
#
# 已知别扭（等再遇到同类问题再一起改造）：产品 feature 只能追加参数，碰上这种
# 单值选项就得把整串 -L 重复一遍，以后加依赖要改两处。备选改法见
# docs/FEATURES.md §2.1「已知别扭」。

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
            configure = [
                "--with-ld-opt=-L{dep:openssl}/lib -L{dep:pcre2}/lib -L{dep:zlib}/lib -L{dep:libxcrypt}/lib",
            ],
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
