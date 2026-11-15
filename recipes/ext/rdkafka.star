LINES = {
    "6.0": {
        "php": ["7.4", "8.0", "8.2", "8.3", "8.4", "8.5", "8.6"],
        "librdkafka": ["2.16"],
    },
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_builtin"]

    return ext(
        name = "rdkafka",
        modules = ["rdkafka"],
        link = ["shared"],
        needs = ["librdkafka"],
        tools = ["autoconf"],
        configure = ["--with-rdkafka={dep:librdkafka}"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-rdkafka.ini",
        desc = "Kafka 客户端扩展，依赖 librdkafka",
    )


TARGET = target
