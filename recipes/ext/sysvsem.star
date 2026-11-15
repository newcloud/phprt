def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "sysvsem",
        modules = ["sysvsem"],
        link = ["static"],
        configure = ["--enable-sysvsem"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "System V 信号量扩展",
    )


TARGET = target
