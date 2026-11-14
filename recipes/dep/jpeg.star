def target(ctx):
    self = ctx["self"]

    return dep(
        name = "jpeg",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        features = {
            "simd": feature(
                configure = [
                    "-DWITH_SIMD=ON",
                    "-DREQUIRE_SIMD=ON",
                ],
                disable = ["-DWITH_SIMD=OFF"],
                requires = {"commands": ["nasm"]},
            ),
        },
        configure = [
            "cmake",
            "-S", "{build}",
            "-B", "{build}",
            "-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_INSTALL_INCLUDEDIR=include",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
            "-DENABLE_SHARED=ON",
            "-DENABLE_STATIC=OFF",
            "-DWITH_TURBOJPEG=OFF",
        ],
        make = [
            "cmake --build {build} --parallel",
            "cmake --install {build}",
        ],
        artifacts = ["lib/libjpeg.so"],
        desc = "libjpeg-turbo 库，供 gd 的 JPEG 支持使用；simd feature 打开 SIMD 加速",
    )


TARGET = target
