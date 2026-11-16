# nginx 1.29 系列共用的产品 profile。
# 组件模块折叠进 nginx 的 ./configure：builtin 用 --add-module，dynamic 用
# --add-dynamic-module。

PROFILE = product_profile(
    tools = [
        tool("gcc", min = "8"),
        tool("make", min = "4.0"),
        "ar",
        "ranlib",
        tool("perl", min = "5.20"),
        tool("patchelf", min = "0.13"),
        "sh",
    ],
    modes = {
        "builtin": mode(
            strategy = "fold",
            configure = ["--add-module={component_source}"],
        ),
        "dynamic": mode(
            strategy = "fold",
            configure = ["--add-dynamic-module={component_source}"],
            artifacts = ["modules/*.so"],
        ),
    },
    defaults = {"bundled": "builtin", "external": "dynamic"},
    runtime = runtime(
        lib_dir = "lib/phprt",
        rpath = {
            "sbin/nginx": "$ORIGIN/../lib/phprt",
            "modules/*.so": "$ORIGIN/../lib/phprt",
        },
    ),
    verify = ["{tree}/sbin/nginx", "-V"],
    entry = "",
    env = {
        "CC": "gcc",
        "CXX": "g++",
    },
    audit = "none",
)
