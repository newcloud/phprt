def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]
    flags = ["--with-curl={dep:libcurl}"]

    return ext(
        name = "curl",
        modules = ["curl"],
        link = ["static", "shared"],
        needs = ["libcurl"],
        tools = ["autoconf"],
        dep_configure = {"libcurl": []},
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        configure = flags,
        shared_from = "ext/curl",
        ini = "20-curl.ini",
        desc = "PHP curl 扩展，依赖功能转交给 libcurl",
    )


TARGET = target
