PHP_DEP_LINES = {
    series: {"oniguruma": ["6.9"]}
    for series in ["7.4", "8.0", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "mbstring",
        modules = ["mbstring"],
        link = ["static"],
        needs = ["oniguruma"],
        configure = ["--enable-mbstring", "--enable-mbregex"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "多字节字符串扩展，mbregex 用树内 oniguruma",
    )


TARGET = target
