# lua-nginx-module 的上游查询规则：git info/refs 列 tag + tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 v0.10.31；v0.10.29R1 / v0.10.32rc5 这类后缀不是纯数字，link
# 正则要求 tag 后直接换行，不进候选。线归属和版本比较在 Rust 侧。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/openresty/lua-nginx-module.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/(?P<link>v[0-9][0-9.]*)\n',
    "version": r'^v(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/openresty/lua-nginx-module/archive/refs/tags/{link}.tar.gz",
}
