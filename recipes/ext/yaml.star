LINES = {
    "2.3": {"php": ["7.4", "8.0", "8.2", "8.3", "8.4", "8.5", "8.6"], "libyaml": ["0.2"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_builtin"]

    return ext(
        name = "yaml",
        modules = ["yaml"],
        link = ["shared"],
        needs = ["libyaml"],
        tools = ["autoconf"],
        configure = ["--with-yaml={dep:libyaml}"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "26-yaml.ini",
        desc = "YAML 解析扩展，依赖 libyaml",
    )


TARGET = target
