PHP_DEP_LINES = {
    series: {"sqlite": ["3.54"]}
    for series in ["7.4", "8.0", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "pdo_sqlite",
        modules = ["pdo_sqlite"],
        link = ["static"],
        needs = ["sqlite"],
        configure = ["--with-pdo-sqlite={dep:sqlite}"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "PDO SQLite 驱动，和 sqlite3 共用同一份 SQLite",
    )


TARGET = target
