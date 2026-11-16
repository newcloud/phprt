LINES = {
    "3.1": {"product": ["7.4", "8.0", "8.1"]},
    "3.5": {"product": ["8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        product = "php",
        name = "xdebug",
        modules = ["xdebug"],
        modes = ["shared"],
        tools = ["autoconf"],
        ignored = {
            "--enable-xdebug": "phpize 默认编译该扩展，不需要显式开关",
            "--enable-xdebug-dev": "调试用途：开发者构建开关",
            "--with-xdebug-compression": "缺依赖（未排期）：zlib",
        },
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "30-xdebug.ini",
        ini_lines = ["zend_extension=xdebug.so"],
        desc = "Xdebug 扩展，契约只支持动态编译",
    )


TARGET = target
