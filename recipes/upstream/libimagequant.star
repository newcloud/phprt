# libimagequant 的上游查询规则：git info/refs 列 tag + tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 是纯版本号（4.4.1），下载地址按 archive/refs/tags/<tag>.tar.gz 拼。
# 线归属和版本比较在 Rust 侧。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/ImageOptim/libimagequant.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/(?P<link>[0-9][0-9.]*)\n',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)+)$',
    "join": "https://github.com/ImageOptim/libimagequant/archive/refs/tags/{link}.tar.gz",
}
