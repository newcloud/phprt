def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libde265",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        tools = ["cmake"],
        needs = [],
        configure = [
            "cmake",
            "-S", "{build}",
            "-B", "{build}",
            "-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
            "-DBUILD_SHARED_LIBS=ON",
            "-DENABLE_DECODER=ON",
            "-DENABLE_ENCODER=OFF",
            "-DENABLE_SDL=OFF",
            "-DENABLE_SHERLOCK265=OFF",
            "-DENABLE_INTERNAL_DEVELOPMENT_TOOLS=OFF",
            "-DWITH_FUZZERS=OFF",
        ],
        make = [
            "cmake --build {build}",
            "cmake --install {build}",
        ],
        artifacts = ["lib/libde265.so"],
        desc = "libde265 HEVC 解码库，供 libheif 解码 HEIC 使用",
    )


TARGET = target
