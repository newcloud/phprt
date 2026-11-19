def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libtiff",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        tools = ["cmake"],
        # JPEG / Deflate 用树内已经编好的库，避免探测系统库；
        # 其余压缩后端（WebP / LZMA / ZSTD / JBIG / libdeflate）显式关闭。
        needs = ["jpeg", "zlib"],
        configure = [
            "cmake",
            "-S", "{build}",
            "-B", "{build}",
            "-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
            "-DCMAKE_PREFIX_PATH={dep:jpeg};{dep:zlib}",
            "-DBUILD_SHARED_LIBS=ON",
            "-Dtiff-static=OFF",
            "-Dtiff-tools=OFF",
            "-Dtiff-tests=OFF",
            "-Dtiff-contrib=OFF",
            "-Dtiff-docs=OFF",
            "-Dtiff-cxx=OFF",
            "-Djpeg=ON",
            "-Dold-jpeg=ON",
            "-Dzlib=ON",
            "-Dpixarlog=ON",
            "-Dlibdeflate=OFF",
            "-Dwebp=OFF",
            "-Dlzma=OFF",
            "-Dzstd=OFF",
            "-Djbig=OFF",
        ],
        make = [
            "cmake --build {build}",
            "cmake --install {build}",
        ],
        artifacts = ["lib/libtiff.so"],
        desc = "libtiff 库，供 gd 的 TIFF 支持使用；JPEG / Deflate 用树内依赖",
    )


TARGET = target
