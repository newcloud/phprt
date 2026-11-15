def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "mysqli",
        modules = ["mysqli"],
        link = ["static"],
        configure = ["--with-mysqli=mysqlnd"],
        ignored = {
            # mysqlnd 模式下 socket 路径用编译期默认值，不需要指定。
            "--with-mysql-sock": "少用：mysqlnd 用编译期默认 socket 路径",
        },
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "MySQLi 扩展，mysqlnd 由 PHP 隐式编入",
    )


TARGET = target
