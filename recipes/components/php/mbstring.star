PRODUCT_DEP_LINES = {
    series: {"oniguruma": ["6.9"]}
    for series in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "mbstring",
        modules = ["mbstring"],
        modes = ["static", "shared"],
        # shared 走 standalone / phpize，需要 autoconf；static 只用不到。
        tools = [tool("autoconf", min = "2.68")],
        needs = ["oniguruma"],
        configure = ["--enable-mbstring", "--enable-mbregex"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-mbstring.ini",
        desc = "多字节字符串扩展，mbregex 用树内 oniguruma",
    )


TARGET = target
