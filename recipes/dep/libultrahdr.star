def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libultrahdr",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        tools = ["cmake"],
        needs = ["jpeg"],
        # UHDR_WRITE_XMP 必须是 OFF：PHP 8.6 的 gd 会检查 libuhdr 的这个编译选项。
        # HEIF/AVIF 支持关掉，只保留 gd 需要的部分。
        configure = [
            "cmake",
            "-S", "{build}",
            "-B", "{build}",
            "-DCMAKE_INSTALL_PREFIX={prefix}",
            "-DCMAKE_INSTALL_LIBDIR=lib",
            "-DCMAKE_BUILD_TYPE=Release",
            "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
            "-DCMAKE_PREFIX_PATH={dep:jpeg}",
            "-DBUILD_SHARED_LIBS=ON",
            "-DUHDR_BUILD_EXAMPLES=OFF",
            "-DUHDR_BUILD_TESTS=OFF",
            "-DUHDR_BUILD_BENCHMARK=OFF",
            "-DUHDR_BUILD_FUZZERS=OFF",
            "-DUHDR_BUILD_DEPS=OFF",
            "-DUHDR_BUILD_JAVA=OFF",
            "-DUHDR_BUILD_PACKAGING=OFF",
            "-DUHDR_ENABLE_HEIF=OFF",
            "-DUHDR_ENABLE_GLES=OFF",
            "-DUHDR_ENABLE_WERROR=OFF",
            "-DUHDR_WRITE_XMP=OFF",
        ],
        make = [
            "cmake --build {build}",
            "cmake --install {build}",
        ],
        artifacts = ["lib/libuhdr.so"],
        desc = "libultrahdr 库，供 gd 的 UltraHDR 支持使用",
    )


TARGET = target
