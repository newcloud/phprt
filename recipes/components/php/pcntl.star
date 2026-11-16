def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "pcntl",
        modules = ["pcntl"],
        modes = ["static"],
        configure = ["--enable-pcntl"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "进程控制扩展（fork/signal），仅 CLI/CGI",
    )


TARGET = target
