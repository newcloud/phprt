def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "sysvshm",
        modules = ["sysvshm"],
        modes = ["static", "shared"],
        # shared 走 standalone / phpize，需要 autoconf；static 只用不到。
        tools = [tool("autoconf", min = "2.68")],
        configure = ["--enable-sysvshm"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-sysvshm.ini",
        desc = "System V 共享内存扩展",
    )


TARGET = target
