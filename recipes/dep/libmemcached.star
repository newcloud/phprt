def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libmemcached",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        tools = ["cmake"],
        needs = [],
        # SASL、memaslap、文档和测试都用不到，关掉减少外部探测。
        configure = [
            "cmake",
            "-S", "{build}",
            "-B", "{build}",
            "-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
            "-DBUILD_SHARED_LIBS=ON",
            "-DBUILD_TESTING=OFF",
            "-DBUILD_DOCS=OFF",
            "-DBUILD_DOCS_MAN=OFF",
            "-DBUILD_DOCS_HTML=OFF",
            "-DENABLE_SASL=OFF",
            "-DENABLE_MEMASLAP=OFF",
            "-DENABLE_OPENSSL_CRYPTO=OFF",
        ],
        make = [
            "cmake --build {build}",
            "cmake --install {build}",
        ],
        artifacts = ["lib/libmemcached.so"],
        desc = "libmemcached 库（awesomized 分支），供 PHP memcached 扩展链接",
    )


TARGET = target
