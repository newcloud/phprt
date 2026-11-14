def target(ctx):
    self = ctx["self"]

    return dep(
        name = "aom",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        configure = [
            "cmake",
            "-S", "{build}",
            "-B", "{build}/aom-build",
            "-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
            "-DBUILD_SHARED_LIBS=ON",
            "-DENABLE_TESTS=OFF",
            "-DENABLE_EXAMPLES=OFF",
            "-DENABLE_DOCS=OFF",
            "-DENABLE_TOOLS=OFF",
            "-DCONFIG_AV1_ENCODER=1",
            "-DCONFIG_AV1_DECODER=1",
            "-DCONFIG_MULTITHREAD=1",
        ],
        make = [
            "cmake --build {build}/aom-build --parallel",
            "cmake --install {build}/aom-build",
        ],
        artifacts = ["lib/libaom.so"],
        desc = "libaom AV1 编解码库，供 libavif 的 AVIF 支持使用",
    )


TARGET = target
