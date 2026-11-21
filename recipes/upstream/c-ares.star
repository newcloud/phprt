# c-ares 的上游查询规则：git info/refs 列 tag + release 资产地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 v1.34.9，文件名前缀是 c-ares-；.asc 和 .intoto.jsonl 不取。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/c-ares/c-ares.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/v(?P<link>[0-9][0-9.]*)\n',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/c-ares/c-ares/releases/download/v{link}/c-ares-{link}.tar.gz",
}
