TARGET = ext(
    name = "openssl",
    origin = "builtin",
    version = "8.5.12",
    modules = ["openssl"],
    link = ["static", "shared"],
    needs = ["openssl"],
    configure = ["--with-openssl={dep:openssl}"],
    shared_from = "ext/openssl",
    ini = "20-openssl.ini",
    desc = "PHP openssl 扩展，可静态编入 PHP 或编成 openssl.so",
)
