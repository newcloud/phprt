LINES = {
    "3.4": {
        # PHP 8.6 删除了 XtOffsetOf / zval_dtor，3.4.0 编不过；等上游支持再加回。
        "product": ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5"],
        "libmemcached": ["1.1"],
        "zlib": ["1.3"],
    },
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        product = "php",
        name = "memcached",
        modules = ["memcached"],
        modes = ["shared"],
        needs = ["libmemcached", "zlib"],
        tools = [tool("autoconf", min = "2.68")],
        ignored = {
            "--enable-memcached-igbinary": "少用：需要 igbinary 扩展；序列化用 PHP 自带实现",
            "--enable-memcached-msgpack": "少用：需要 msgpack 扩展；序列化用 PHP 自带实现",
            "--enable-memcached-protocol": "少用：libmemcached 的二进制协议服务端支持，PHP 扩展用不到",
            "--with-system-fastlz": "少用：扩展自带 FastLZ，不需要系统库",
            "--with-zstd": "缺依赖（未排期）：zstd",
            "--enable-debug": "调试用途：输出 libmemcached 调试信息",
        },
        configure = [
            "--enable-memcached",
            "--with-libmemcached-dir={dep:libmemcached}",
            "--with-zlib-dir={dep:zlib}",
            "--disable-memcached-sasl",
            "--enable-memcached-session",
            "--enable-memcached-json",
        ],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-memcached.ini",
        desc = "Memcached 客户端扩展，支持 session 和 JSON 序列化",
    )


TARGET = target
