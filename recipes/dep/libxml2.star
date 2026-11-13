def target(ctx):
    self = ctx["self"]

    return dep(
        name = "libxml2",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        # PHP 的 libxml 只用解析器和树模型，不遍历文档、也不需要 Python 绑定。
        # 可选外部库显式关闭，避免 configure 探测到系统里的同名库。
        configure = [
            "./configure",
            "--prefix={prefix}",
            "--disable-static",
            "--without-zlib",
            "--without-icu",
            "--without-readline",
            "--without-python",
        ],
        make = ["make", "make install"],
        artifacts = ["lib/libxml2.so.16"],
        desc = "libxml2 库，供 PHP libxml 扩展链接",
    )


TARGET = target
