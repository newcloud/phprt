LINES = {
    "6.0": {
        # PHP 8.6 删除了 XtOffsetOf，6.0.5 编不过；等上游支持再加回。
        "product": ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5"],
        "librdkafka": ["2.16"],
    },
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        product = "php",
        name = "rdkafka",
        modules = ["rdkafka"],
        modes = ["shared"],
        needs = ["librdkafka"],
        tools = [tool("autoconf", min = "2.68")],
        configure = ["--with-rdkafka={dep:librdkafka}"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-rdkafka.ini",
        desc = "Kafka 客户端扩展，依赖 librdkafka",
    )


TARGET = target
