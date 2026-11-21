# libmemcached 的上游查询规则：git info/refs 列 tag + tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 主要是纯版本号（1.1.5）；_11 / pre_cmake 这类非版本 tag 被正则排除。
# 下载地址按 archive/refs/tags/<tag>.tar.gz 拼。线归属和版本比较在 Rust 侧。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/awesomized/libmemcached.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/(?P<link>[0-9][0-9.]*)\n',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)+)$',
    "join": "https://github.com/awesomized/libmemcached/archive/refs/tags/{link}.tar.gz",
}
