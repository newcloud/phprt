PRODUCT_DEP_LINES = {
    series: {"libsodium": ["1.0"]}
    for series in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "sodium",
        modules = ["sodium"],
        modes = ["static", "shared"],
        # shared 走 standalone / phpize，需要 autoconf；static 只用不到。
        tools = [tool("autoconf", min = "2.68")],
        needs = ["libsodium"],
        configure = ["--with-sodium={dep:libsodium}"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-sodium.ini",
        desc = "libsodium 密码学扩展",
    )


TARGET = target
