def target(ctx):
    self = ctx["self"]
    line = self["line"]

    # libxml2 2.12 改了 xmlStructuredErrorFunc 的 const 签名，并删掉 xmlversion.h 里的
    # ATTRIBUTE_UNUSED 宏；PHP 8.0 / 7.4 分支 EOL 后没拿到上游修复，编不过 2.12+，
    # 只能钉 2.11 线，其余系列用 2.15。两条线的 soname 不同：2.11 是 .so.2，2.15 是 .so.16。
    if line == "2.11":
        artifacts = ["lib/libxml2.so.2"]
    else:
        artifacts = ["lib/libxml2.so.16"]

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
        artifacts = artifacts,
        desc = "libxml2 库，供 PHP libxml 扩展链接",
    )


TARGET = target
