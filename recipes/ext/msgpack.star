LINES = {
    "3.0": {"php": ["7.4", "8.0", "8.2", "8.3", "8.4", "8.5", "8.6"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_builtin"]

    return ext(
        name = "msgpack",
        modules = ["msgpack"],
        link = ["shared"],
        tools = ["autoconf"],
        # 源码自带 bundled msgpack-c，不需要外部库。
        configure = ["--with-msgpack"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-msgpack.ini",
        desc = "MessagePack 序列化扩展（自带 msgpack-c）",
    )


TARGET = target
