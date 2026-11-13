PHP_LINES = {
    "7.4": ["1.1.1"],
    "8.0": ["1.1.1"],
    "8.2": ["1.1.1", "3.0", "3.5"],
    "8.3": ["1.1.1", "3.0", "3.5"],
    "8.4": ["1.1.1", "3.0", "3.5"],
    "8.5": ["1.1.1", "3.0", "3.5", "4.0"],
    "8.6": ["1.1.1", "3.0", "3.5", "4.0"],
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_builtin"]

    if ctx["php_series"] == "7.4":
        flags = ["--with-openssl-dir={dep:openssl}"]
    else:
        flags = ["--with-openssl={dep:openssl}"]

    # --with-openssl-argon2 是 PHP 8.5+ 的 configure 选项，要求 OpenSSL >= 3.2；
    # 条件写在 requires 里，由 Rust 校验并给出可读的报错。
    features = {
        "argon2": feature(
            configure = ["--with-openssl-argon2"],
            requires = {
                "php": ["8.5", "8.6"],
                "dep": {"openssl": ["3.5", "4.0"]},
            },
        ),
    }

    return ext(
        name = "openssl",
        modules = ["openssl"],
        link = ["static", "shared"],
        needs = ["openssl"],
        tools = ["autoconf"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        features = features,
        configure = flags,
        shared_from = "ext/openssl",
        ini = "20-openssl.ini",
        desc = "PHP openssl 扩展，可静态编入 PHP 或编成 openssl.so",
    )


TARGET = target
