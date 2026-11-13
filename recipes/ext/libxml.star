def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "libxml",
        modules = ["libxml"],
        # PHP 的 libxml 扩展不支持 shared（上游 config0.m4 里 ext_shared=no）。
        link = ["static"],
        needs = ["libxml2"],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        # 上游没有目录参数，libxml2 由 pkg-config 定位；显式指定让关闭参数
        # --without-libxml 在选中时被替换成成对的正向参数。
        configure = ["--with-libxml"],
        desc = "PHP libxml 扩展，静态编入 PHP；DOM/SimpleXML 等扩展的基础",
    )


TARGET = target
