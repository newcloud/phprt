LINES = {
    "4.8": {"php": ["7.4", "8.0", "8.2"], "openssl": ["1.1.1"]},
    "5.1": {"php": ["8.0", "8.2", "8.3"], "openssl": ["1.1.1", "3.0", "3.5"]},
    "6.0": {"php": ["8.2", "8.3", "8.4"], "openssl": ["1.1.1", "3.0", "3.5"]},
    "6.2": {"php": ["8.4", "8.5", "8.6"], "openssl": ["1.1.1", "3.0", "3.5", "4.0"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_builtin"]
    line = self["line"]

    needs = ["libcurl"]
    configure = []
    if "openssl" in LINES[line]:
        needs = ["libcurl", "openssl"]
        if line in ("6.0", "6.2"):
            # swoole 6.x 的 SSL 代码无条件编译，上游已移除 --enable-openssl。
            configure = [
                "--enable-swoole",
                "--with-openssl-dir={dep:openssl}",
                "--disable-brotli",
            ]
        else:
            configure = ["--enable-openssl"]

    return ext(
        name = "swoole",
        modules = ["swoole"],
        link = ["shared"],
        needs = needs,
        tools = ["autoconf"],
        subs = {"curl": sub(dep = "libcurl", configure = ["--enable-swoole-curl"])},
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        configure = configure,
        ini = "27-swoole.ini",
        desc = "Swoole 扩展，契约只支持动态编译",
    )


TARGET = target
