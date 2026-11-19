LINES = {
    # PHP 8.6 删除了 zval_dtor、改了 zend_string 相关签名，3.2.16 编不过；等上游支持再加回。
    "3.2": {"product": ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    # PHP 8.5 删除了 ext/standard/php_smart_string.h；igbinary 3.2.16 只是
    # include 了它，实际没有用到 smart_str/smart_string，删掉这一行即可编译。
    # 以后把 LINES 扩到 8.6 时，这里要一起加上。
    prepare = []
    if ctx["product_series"] in ("8.5",):
        prepare = [
            "sed -i '\\|#include.*php_smart_string.h|d' src/php7/php_igbinary.h",
        ]

    return component(
        product = "php",
        name = "igbinary",
        modules = ["igbinary"],
        modes = ["shared"],
        tools = [tool("autoconf", min = "2.68")],
        prepare = prepare,
        configure = ["--enable-igbinary"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-igbinary.ini",
        desc = "igbinary 序列化扩展，PHP 8.5 构建前删除已移除的头文件 include",
    )


TARGET = target
