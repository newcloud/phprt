def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "sockets",
        modules = ["sockets"],
        modes = ["static"],
        configure = ["--enable-sockets"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "BSD socket 扩展",
    )


TARGET = target
