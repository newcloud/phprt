PRODUCT_LINES = {
    "7.4": ["1.1.1"],
    "8.0": ["1.1.1"],
    "8.1": ["1.1.1", "3.0", "3.5"],
    "8.2": ["1.1.1", "3.0", "3.5"],
    "8.3": ["1.1.1", "3.0", "3.5"],
    "8.4": ["1.1.1", "3.0", "3.5"],
    "8.5": ["1.1.1", "3.0", "3.5", "4.0"],
    "8.6": ["1.1.1", "3.0", "3.5", "4.0"],
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    if ctx["product_series"] == "7.4":
        flags = ["--with-openssl-dir={dep:openssl}"]
    else:
        flags = ["--with-openssl={dep:openssl}"]

    # --with-openssl-argon2 是 PHP 8.4+ 的 configure 选项，要求 OpenSSL >= 3.2；
    # 条件写在 requires 里，由 Rust 校验并给出可读的报错。
    features = {
        "argon2": feature(
            configure = ["--with-openssl-argon2"],
            requires = {
                "product": ["8.4", "8.5", "8.6"],
                "dep": {"openssl": ["3.5", "4.0"]},
            },
        ),
        # 老证书、MD4/RC4/DES 这类遗留算法需要加载 legacy provider。
        # 默认关：它会降低安全边界，只在对接旧系统时显式打开。
        "legacy-provider": feature(
            configure = ["--with-openssl-legacy-provider"],
            requires = {
                "product": ["8.4", "8.5", "8.6"],
                "dep": {"openssl": ["3.0", "3.5", "4.0"]},
            },
        ),
    }

    # 上游选项里看过、但暂不暴露的开关。
    ignored = {
        "--with-system-ciphers": "少用：依赖系统 cipher 配置，构建不可复现",
    }
    # 8.0 / 8.1 / 8.2 / 8.3 有这个选项，8.4 起上游已删除。
    if ctx["product_series"] in ("8.0", "8.1", "8.2", "8.3"):
        ignored["--with-kerberos"] = "少用：需要系统 krb5 库，只在对接 Kerberos 认证时用"

    return component(
        product = "php",
        name = "openssl",
        modules = ["openssl"],
        modes = ["static", "shared"],
        needs = ["openssl"],
        tools = ["autoconf"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        features = features,
        ignored = ignored,
        configure = flags,
        source_path = "ext/openssl",
        ini = "20-openssl.ini",
        desc = "PHP openssl 扩展，可静态编入 PHP 或编成 openssl.so",
    )


TARGET = target
