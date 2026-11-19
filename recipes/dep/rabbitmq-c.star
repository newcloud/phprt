def target(ctx):
    self = ctx["self"]

    return dep(
        name = "rabbitmq-c",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        tools = ["cmake"],
        needs = ["openssl"],
        configure = [
            "cmake",
            "-S", "{build}",
            "-B", "{build}",
            "-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
            "-DBUILD_SHARED_LIBS=ON",
            "-DBUILD_EXAMPLES=OFF",
            "-DBUILD_TESTS=OFF",
            "-DBUILD_TOOLS=OFF",
            "-DBUILD_DOCS=OFF",
            "-DENABLE_SSL_SUPPORT=ON",
            "-DOPENSSL_ROOT_DIR={dep:openssl}",
        ],
        make = [
            "cmake --build {build}",
            "cmake --install {build}",
        ],
        artifacts = ["lib/librabbitmq.so"],
        desc = "rabbitmq-c 库，供 PHP amqp 扩展链接",
    )


TARGET = target
