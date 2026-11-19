LINES = {
    "5.36": {"product": ["8.2", "8.3", "8.4", "8.5", "8.6"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        product = "php",
        name = "protobuf",
        modules = ["protobuf"],
        modes = ["shared"],
        tools = [tool("autoconf", min = "2.68")],
        # 纯 C 实现，自带精简版 upb，不依赖外部 libprotobuf。
        configure = ["--enable-protobuf"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "23-protobuf.ini",
        desc = "Protocol Buffers 扩展（自带 upb）",
    )


TARGET = target
