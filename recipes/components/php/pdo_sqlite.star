PRODUCT_DEP_LINES = {
    series: {"sqlite": ["3.54"]}
    for series in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "pdo_sqlite",
        modules = ["pdo_sqlite"],
        modes = ["static", "shared"],
        # shared 走 standalone / phpize，需要 autoconf；static 只用不到。
        tools = [tool("autoconf", min = "2.68")],
        needs = ["sqlite"],
        dep_configure = {"sqlite": []},
        configure = ["--with-pdo-sqlite={dep:sqlite}"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-pdo_sqlite.ini",
        desc = "PDO SQLite 驱动，和 sqlite3 共用同一份 SQLite",
    )


TARGET = target
