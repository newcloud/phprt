# ffi 的依赖叫 libffi，和组件不同名，所以用 PRODUCT_DEP_LINES。
# 8 个系列都必须写：内置组件只要有了 recipe 就一定参与约束，
# 缺一个系列，那个系列即使不选 ffi 也会在 bind 阶段失败。
PRODUCT_DEP_LINES = {
    series: {"libffi": ["3.8"]}
    for series in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_bundled"]
    self = ctx["self"]

    return component(
        product = "php",
        name = "ffi",
        modules = ["ffi"],
        modes = ["static", "shared"],
        needs = ["libffi"],
        # shared 走 phpize，需要 autoconf；static 只用不到。
        tools = [tool("autoconf", min = "2.68")],
        # ext/ffi/config.m4 用 PKG_CHECK_MODULES 查 libffi >= 3.0.11，
        # --with-ffi 不接受目录参数；路径由构建环境的 PKG_CONFIG_PATH 注入。
        configure = ["--with-ffi"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        source_path = "ext/ffi",
        ini = "20-ffi.ini",
        desc = "PHP FFI 扩展，依赖 libffi",
    )


TARGET = target
