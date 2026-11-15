PHP_DEP_LINES = {
    series: {"sqlite": ["3.54"]}
    for series in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "sqlite3",
        modules = ["sqlite3"],
        link = ["static"],
        needs = ["sqlite"],
        dep_configure = {"sqlite": []},
        configure = ["--with-sqlite3={dep:sqlite}"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "SQLite3 扩展",
    )


TARGET = target
