TARGET = ext(
    name = "curl",
    origin = "builtin",
    version = "8.5.12",
    modules = ["curl"],
    link = ["static", "shared"],
    needs = ["libcurl"],
    subs = {"curl": sub(dep = "libcurl")},
    configure = ["--with-curl={dep:libcurl}"],
    shared_from = "ext/curl",
    ini = "20-curl.ini",
    desc = "PHP curl 扩展，子功能转交给 libcurl",
)
