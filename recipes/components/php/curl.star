# libcurl 8.23 要 OpenSSL >= 3.0；PHP 8.0 的 openssl 扩展编不过 OpenSSL 3.x，
# 所以 7.4 / 8.0 只能用 7.88 线配 openssl 1.1.1，其余系列用 8.23。
PRODUCT_DEP_LINES = {
    "7.4": {"libcurl": ["7.88"]},
    "8.0": {"libcurl": ["7.88"]},
    "8.1": {"libcurl": ["8.23"]},
    "8.2": {"libcurl": ["8.23"]},
    "8.3": {"libcurl": ["8.23"]},
    "8.4": {"libcurl": ["8.23"]},
    "8.5": {"libcurl": ["8.23"]},
    "8.6": {"libcurl": ["8.23"]},
}


def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]
    flags = ["--with-curl={dep:libcurl}"]

    return component(
        product = "php",
        name = "curl",
        modules = ["curl"],
        modes = ["static", "shared"],
        needs = ["libcurl"],
        tools = ["autoconf"],
        dep_configure = {"libcurl": []},
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        configure = flags,
        source_path = "ext/curl",
        ini = "20-curl.ini",
        desc = "PHP curl 扩展，依赖功能转交给 libcurl",
    )


TARGET = target
