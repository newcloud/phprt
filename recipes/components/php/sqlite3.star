PRODUCT_DEP_LINES = {
    series: {"sqlite": ["3.54"]}
    for series in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    # PHP 源码里的 ext/sqlite3 只有 config0.m4，phpize 只认 config.m4。
    # 这是 PHP recipe 的源码准备动作，通用构建执行器不再按文件名自动处理。
    prepare = (
        ["test -f config.m4 || cp config0.m4 config.m4"]
        if builtin
        else []
    )

    return component(
        product = "php",
        name = "sqlite3",
        modules = ["sqlite3"],
        modes = ["static", "shared"],
        needs = ["sqlite"],
        # shared 走 standalone / phpize，需要 autoconf；static 只用不到。
        tools = [tool("autoconf", min = "2.68")],
        prepare = prepare,
        dep_configure = {"sqlite": []},
        configure = ["--with-sqlite3={dep:sqlite}"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-sqlite3.ini",
        desc = "SQLite3 扩展",
    )


TARGET = target
