PHP_DEP_LINES = {
    series: {"libsodium": ["1.0"]}
    for series in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "sodium",
        modules = ["sodium"],
        link = ["static"],
        needs = ["libsodium"],
        configure = ["--with-sodium={dep:libsodium}"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        desc = "libsodium 密码学扩展",
    )


TARGET = target
