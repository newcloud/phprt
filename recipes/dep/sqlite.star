def target(ctx):
    self = ctx["self"]

    return dep(
        name = "sqlite",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-static",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libsqlite3.so"],
        desc = "SQLite 库，供 PHP sqlite3 / pdo_sqlite 扩展链接",
    )


TARGET = target
