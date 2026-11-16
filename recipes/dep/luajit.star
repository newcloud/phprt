def target(ctx):
    self = ctx["self"]

    # LuaJIT 的 Makefile 不适合并行构建，这里显式 -j1。
    return dep(
        name = "luajit",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        make = ["make -j1 PREFIX={prefix} install"],
        artifacts = ["lib/libluajit-5.1.so"],
        desc = "OpenResty LuaJIT2，供 lua-nginx-module 使用",
    )


TARGET = target
