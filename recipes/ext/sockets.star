def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "sockets",
        modules = ["sockets"],
        link = ["static"],
        configure = ["--enable-sockets"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "BSD socket 扩展",
    )


TARGET = target
