def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "sysvmsg",
        modules = ["sysvmsg"],
        modes = ["static"],
        configure = ["--enable-sysvmsg"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "System V 消息队列扩展",
    )


TARGET = target
