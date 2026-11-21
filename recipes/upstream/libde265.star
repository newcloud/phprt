# libde265 的上游查询规则：git info/refs 列 tag + release 资产地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 v1.1.3，每版一个 .tar.gz asset，文件名前缀是 libde265-。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/strukturag/libde265.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/v(?P<link>[0-9][0-9.]*)\n',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/strukturag/libde265/releases/download/v{link}/libde265-{link}.tar.gz",
}
