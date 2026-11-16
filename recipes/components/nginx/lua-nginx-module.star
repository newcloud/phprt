LINES = {
    "0.10": {
        "product": ["1.29"],
        "luajit": ["2.1"],
        "lua-resty-core": ["0.1"],
        "lua-resty-lrucache": ["0.15"],
    },
}


def target(ctx):
    self = ctx["self"]

    return component(
        name = "lua-nginx-module",
        product = "nginx",
        source = "external",
        modes = ["dynamic"],
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = ["luajit", "lua-resty-core", "lua-resty-lrucache"],
        env = {
            "LUAJIT_INC": "{dep:luajit}/include/luajit-2.1",
            "LUAJIT_LIB": "{dep:luajit}/lib",
        },
        desc = "OpenResty lua-nginx-module，动态模块；依赖 LuaJIT",
    )


TARGET = target
