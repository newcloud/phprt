TARGET = ext(
    name = "swoole",
    origin = "pecl",
    version = "6.0.2",
    modules = ["swoole"],
    link = ["shared"],
    needs = ["libcurl"],
    subs = {"curl": sub(dep = "libcurl", configure = ["--enable-swoole-curl"])},
    url = "https://pecl.php.net/get/swoole-6.0.2.tgz",
    ini = "27-swoole.ini",
    desc = "Swoole 扩展，契约只支持动态编译",
)
