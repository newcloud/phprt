LINES = {
    "6.3": {"product": ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        product = "php",
        name = "redis",
        modules = ["redis"],
        modes = ["shared"],
        tools = ["autoconf"],
        # 默认沿用 redis 自带的序列化实现；lzf 压缩用扩展自带的 liblzf。
        features = {
            "lzf": feature(configure = ["--enable-redis-lzf"]),
        },
        ignored = {
            "--enable-redis-igbinary": "少用：需要 igbinary 扩展；序列化用 redis 自带实现",
            "--enable-redis-msgpack": "少用：需要 msgpack 扩展；序列化用 redis 自带实现",
            "--enable-redis-lz4": "缺依赖（未排期）：liblz4",
            "--with-liblz4": "缺依赖（未排期）：liblz4",
            "--enable-redis-zstd": "缺依赖（未排期）：zstd",
            "--with-libzstd": "缺依赖（未排期）：zstd",
            "--with-liblzf": "少用：扩展自带 liblzf，不需要系统库",
        },
        configure = [
            "--enable-redis",
            "--enable-redis-session",
            "--enable-redis-json",
        ],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-redis.ini",
        desc = "Redis 客户端扩展，支持 session 和 JSON 序列化",
    )


TARGET = target
