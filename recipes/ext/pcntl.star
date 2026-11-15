def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "pcntl",
        modules = ["pcntl"],
        link = ["static"],
        configure = ["--enable-pcntl"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "进程控制扩展（fork/signal），仅 CLI/CGI",
    )


TARGET = target
