def target(ctx):
    self = ctx["self"]

    return tool_target(
        name = "autoconf",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        tools = [tool("m4", min = "1.4")],
        configure = [
            "./configure",
            "--prefix={prefix}",
        ],
        make = ["make", "make install"],
        artifacts = ["bin/autoconf"],
        desc = "构建期工具：phpize 需要 autoconf",
    )


TARGET = target
