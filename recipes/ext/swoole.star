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
        dep_configure = {"libcurl": ["--enable-swoole-curl"]},
        # 生产环境常用的异步 DNS；只在选中时引入 c-ares。
        features = {
            "cares": feature(
                needs = ["c-ares"],
                configure = ["--enable-cares"],
            ),
        },
        # 上游选项里看过、但暂不暴露的开关。要开放时改成 features / dep_configure。
        ignored = {
            "--enable-asan": "调试用途：只用于测试",
            "--enable-debug": "调试用途：不用于发布构建",
            "--enable-debug-log": "调试用途：日志开销大",
            "--enable-iouring": "少用：依赖内核版本和 liburing",
            "--with-liburing-dir": "少用：随 iouring 一起不暴露",
            "--enable-uring-socket": "少用：随 iouring 一起不暴露",
            "--enable-mysqlnd": "少用：需要 PHP mysqlnd 扩展，当前不构建",
            "--enable-sockets": "少用：需要 PHP sockets 扩展",
            "--enable-swoole-coro-time": "少用：上游统计开关",
            "--enable-swoole-coverage": "调试用途：代码覆盖率",
            "--enable-swoole-dev": "调试用途：开发者模式，不用于发布",
            "--enable-swoole-ftp": "少用：需要 FTP 客户端支持",
            "--enable-swoole-pgsql": "少用：需要 pgsql",
            "--enable-swoole-sqlite": "少用：需要 sqlite3",
            "--enable-swoole-stdext": "调试用途：上游内部开关",
            "--enable-swoole-thread": "少用：线程模式，暂未验证",
            "--enable-thread-context": "调试用途：配合线程模式",
            "--enable-trace-log": "调试用途：日志开销大",
            "--enable-zstd": "缺依赖（未排期）：zstd",
            "--with-brotli-dir": "缺依赖（未排期）：brotli",
            "--with-nghttp2-dir": "少用：nghttp2 由 libcurl 提供，swoole 不需要单独指定",
            "--with-swoole-firebird": "少用：需要 firebird",
            "--with-swoole-odbc": "少用：需要 ODBC",
            "--with-swoole-oracle": "少用：需要 Oracle 客户端",
            "--with-swoole-ssh2": "少用：需要 libssh2",
        },
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        configure = configure,
        ini = "27-swoole.ini",
        desc = "Swoole 扩展，契约只支持动态编译",
    )


TARGET = target
