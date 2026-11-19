LINES = {
    # PHP 8.6 删除了 zval_dtor，2.3.0 编不过；等上游支持再加回。
    "2.3": {"product": ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5"], "libyaml": ["0.2"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        product = "php",
        name = "yaml",
        modules = ["yaml"],
        modes = ["shared"],
        needs = ["libyaml"],
        tools = [tool("autoconf", min = "2.68")],
        configure = ["--with-yaml={dep:libyaml}"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "26-yaml.ini",
        desc = "YAML 解析扩展，依赖 libyaml",
    )


TARGET = target
