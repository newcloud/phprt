def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "sysvshm",
        modules = ["sysvshm"],
        link = ["static"],
        configure = ["--enable-sysvshm"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "System V 共享内存扩展",
    )


TARGET = target
