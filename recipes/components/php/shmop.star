def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "shmop",
        modules = ["shmop"],
        modes = ["static"],
        configure = ["--enable-shmop"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "共享内存操作扩展",
    )


TARGET = target
