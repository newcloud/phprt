def target(ctx):
    self = ctx["self"]

    return dep(
        name = "avif",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = ["aom"],
        configure = [
            "cmake",
            "-S", "{build}",
            "-B", "{build}",
            "-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
            "-DCMAKE_PREFIX_PATH={dep:aom}",
            "-DBUILD_SHARED_LIBS=ON",
            "-DAVIF_CODEC_AOM=SYSTEM",
            "-DAVIF_LIBYUV=OFF",
            "-DAVIF_BUILD_APPS=OFF",
            "-DAVIF_BUILD_EXAMPLES=OFF",
            "-DAVIF_BUILD_TESTS=OFF",
            "-DAVIF_BUILD_MAN_PAGES=OFF",
            "-DAVIF_ENABLE_WERROR=OFF",
        ],
        make = [
            "cmake --build {build} --parallel",
            "cmake --install {build}",
        ],
        artifacts = ["lib/libavif.so"],
        desc = "libavif 库，供 gd 的 AVIF 支持使用；AOM 作为 AV1 后端",
    )


TARGET = target
