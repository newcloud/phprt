def target(ctx):
    self = ctx["self"]

    return tool_target(
        name = "patchelf",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        configure = [
            "./configure",
            "--prefix={prefix}",
        ],
        make = ["make", "make install"],
        artifacts = ["bin/patchelf"],
        desc = "构建期工具：设置 ELF 的 rpath",
    )


TARGET = target
