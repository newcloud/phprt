def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "shmop",
        modules = ["shmop"],
        link = ["static"],
        configure = ["--enable-shmop"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "共享内存操作扩展",
    )


TARGET = target
