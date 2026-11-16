LINES = {
    "1.5": {"product": ["8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"], "libssh2": ["1.11"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        product = "php",
        name = "ssh2",
        modules = ["ssh2"],
        modes = ["shared"],
        needs = ["libssh2"],
        tools = ["autoconf"],
        configure = ["--with-ssh2={dep:libssh2}"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-ssh2.ini",
        desc = "SSH2 客户端扩展，依赖 libssh2",
    )


TARGET = target
