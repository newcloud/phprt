# oniguruma 的上游查询规则：git info/refs 列 tag + release 资产地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 v6.9.10；文件名前缀是 onig-（不是 oniguruma-），.sha256 sidecar 不取。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/kkos/oniguruma.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/v(?P<link>[0-9][0-9.]*)\n',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/kkos/oniguruma/releases/download/v{link}/onig-{link}.tar.gz",
}
