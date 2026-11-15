PHP_LINES = {
    series: ["1.3"]
    for series in ["7.4", "8.0", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "zlib",
        modules = ["zlib"],
        link = ["static", "shared"],
        needs = ["zlib"],
        configure = ["--with-zlib={dep:zlib}"],
        shared_from = "ext/zlib",
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-zlib.ini",
        desc = "zlib 压缩扩展，供 phar/gzip 流使用",
    )


TARGET = target
