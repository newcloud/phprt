def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    ignored = {}
    # 只有 8.3 还有这个选项，8.4 起上游已删除。
    if ctx["php_series"] == "8.3":
        ignored["--with-zlib-dir"] = "少用：mysqlnd 压缩走 PHP 的 zlib，不单独指定库路径"

    return ext(
        name = "pdo_mysql",
        modules = ["pdo_mysql"],
        link = ["static"],
        ignored = ignored,
        configure = ["--with-pdo-mysql=mysqlnd"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "PDO MySQL 驱动，mysqlnd 由 PHP 隐式编入",
    )


TARGET = target
