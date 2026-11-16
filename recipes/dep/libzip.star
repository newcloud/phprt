def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libzip",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        host_tools = ["cmake"],
        needs = ["zlib"],
        # 压缩后端只留 zlib；加密后端默认关，打开时用 PHP 自己编的 openssl。
        features = {
            "encryption": feature(
                needs = ["openssl"],
                configure = [
                    "-DENABLE_OPENSSL=ON",
                    "-DOPENSSL_ROOT_DIR={dep:openssl}",
                ],
                disable = ["-DENABLE_OPENSSL=OFF"],
            ),
        },
        configure = [
            "cmake",
            "-S", "{build}",
            "-B", "{build}",
            "-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DBUILD_SHARED_LIBS=ON",
            "-DENABLE_LZMA=OFF",
            "-DENABLE_ZSTD=OFF",
            "-DENABLE_BZIP2=OFF",
            "-DENABLE_GNUTLS=OFF",
            "-DENABLE_COMMONCRYPTO=OFF",
            "-DENABLE_WINDOWS_CRYPTO=OFF",
            "-DBUILD_TOOLS=OFF",
            "-DBUILD_REGRESS=OFF",
            "-DBUILD_EXAMPLES=OFF",
            "-DBUILD_DOC=OFF",
            "-DCMAKE_PREFIX_PATH={dep:zlib}",
        ],
        make = [
            "cmake --build {build} --parallel",
            "cmake --install {build}",
        ],
        artifacts = ["lib/libzip.so"],
        desc = "libzip 库，供 PHP zip 扩展链接",
    )


TARGET = target
