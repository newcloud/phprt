# lua-resty-core 的上游查询规则：git info/refs 列 tag + tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 v1.0.0；带 rc / -RC 后缀的 tag 后面不是换行，不进候选。下载地址
# 按 archive/refs/tags/<tag>.tar.gz 拼。线归属和版本比较在 Rust 侧。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/openresty/lua-resty-core.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/(?P<link>v[0-9][0-9.]*)\n',
    "version": r'^v(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/openresty/lua-resty-core/archive/refs/tags/{link}.tar.gz",
}
