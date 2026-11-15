def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "gettext",
        modules = ["gettext"],
        link = ["static"],
        configure = ["--with-gettext"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "GNU gettext 扩展；glibc 自带 libintl，不需要额外依赖",
    )


TARGET = target
