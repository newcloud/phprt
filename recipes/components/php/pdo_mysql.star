def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    ignored = {}
    # 只有 8.3 还有这个选项，8.4 起上游已删除。
    if ctx["product_series"] == "8.3":
        ignored["--with-zlib-dir"] = "少用：mysqlnd 压缩走 PHP 的 zlib，不单独指定库路径"

    return component(
        product = "php",
        name = "pdo_mysql",
        modules = ["pdo_mysql"],
        modes = ["static"],
        ignored = ignored,
        configure = ["--with-pdo-mysql=mysqlnd"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "PDO MySQL 驱动，mysqlnd 由 PHP 隐式编入",
    )


TARGET = target
