LINES = {
    # PHP 8.6 删除了 XtOffsetOf / INI_INT 等宏，2.2.0 编不过；等上游发支持 8.6 的版本再加回。
    "2.2": {"product": ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5"], "rabbitmq-c": ["0.18"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        product = "php",
        name = "amqp",
        modules = ["amqp"],
        modes = ["shared"],
        needs = ["rabbitmq-c"],
        tools = [tool("autoconf", min = "2.68")],
        configure = [
            "--with-amqp",
            "--with-librabbitmq-dir={dep:rabbitmq-c}",
        ],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-amqp.ini",
        desc = "AMQP 客户端扩展，依赖 rabbitmq-c",
    )


TARGET = target
