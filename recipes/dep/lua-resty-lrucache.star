def target(ctx):
    self = ctx["self"]

    # 纯 Lua 库：装进 LuaJIT 的 share/lua/5.1，resty.core 需要它。
    return dep(
        name = "lua-resty-lrucache",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = ["luajit"],
        make = [
            "make install PREFIX={dep:luajit} LUA_LIB_DIR={dep:luajit}/share/lua/5.1",
        ],
        desc = "lua-resty-lrucache，lua-resty-core 的运行时依赖",
    )


TARGET = target
