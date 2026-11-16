def target(ctx):
    self = ctx["self"]

    # 纯 Lua 库：装进 LuaJIT 的 share/lua/5.1，运行时不用再设 LUA_PATH。
    return dep(
        name = "lua-resty-core",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = ["luajit"],
        make = [
            "make install PREFIX={dep:luajit} LUA_LIB_DIR={dep:luajit}/share/lua/5.1",
        ],
        desc = "lua-resty-core，lua-nginx-module v0.10.16+ 的必需依赖",
    )


TARGET = target
