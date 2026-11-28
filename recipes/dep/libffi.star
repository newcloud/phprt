def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libffi",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        # libffi 的 configure 要求 --libdir 是绝对路径，而 --root 为相对路径时
        # {prefix} 也是相对的；改用 --disable-multi-os-directory 让它不按
        # multi-os 把 .so 放进 lib64，产物固定为 lib/libffi.so。
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-multi-os-directory",
            "--disable-static",
            "--disable-docs",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libffi.so"],
        desc = "libffi 库，供 PHP ffi 扩展链接",
    )


TARGET = target
