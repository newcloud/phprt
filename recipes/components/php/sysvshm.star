def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "sysvshm",
        modules = ["sysvshm"],
        modes = ["static"],
        configure = ["--enable-sysvshm"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "System V 共享内存扩展",
    )


TARGET = target
