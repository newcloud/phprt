# patchelf 的上游查询规则：git info/refs 列 tag + release 资产地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 0.19.3；发布页另有平台变体（patchelf-0.19.3-aarch64.tar.gz），
# 源码包固定是 patchelf-<版本>.tar.gz。写入前由 final_url 探测兜底。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/NixOS/patchelf.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/(?P<link>[0-9][0-9.]*)\n',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/NixOS/patchelf/releases/download/{link}/patchelf-{link}.tar.gz",
}
