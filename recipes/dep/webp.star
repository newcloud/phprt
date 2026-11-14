def target(ctx):
    self = ctx["self"]

    return dep(
        name = "webp",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
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
            "-DWEBP_BUILD_ANIM_UTILS=OFF",
            "-DWEBP_BUILD_CWEBP=OFF",
            "-DWEBP_BUILD_DWEBP=OFF",
            "-DWEBP_BUILD_GIF2WEBP=OFF",
            "-DWEBP_BUILD_IMG2WEBP=OFF",
            "-DWEBP_BUILD_VWEBP=OFF",
            "-DWEBP_BUILD_WEBPINFO=OFF",
            "-DWEBP_BUILD_WEBPMUX=OFF",
            "-DWEBP_BUILD_EXTRAS=OFF",
        ],
        make = [
            "cmake --build {build} --parallel",
            "cmake --install {build}",
        ],
        artifacts = ["lib/libwebp.so"],
        desc = "libwebp 库，供 gd 的 WebP 支持使用",
    )


TARGET = target
