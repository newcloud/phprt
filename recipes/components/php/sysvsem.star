def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "sysvsem",
        modules = ["sysvsem"],
        modes = ["static"],
        configure = ["--enable-sysvsem"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "System V 信号量扩展",
    )


TARGET = target
