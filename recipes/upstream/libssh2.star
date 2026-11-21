# libssh2 的上游查询规则：git info/refs 列 tag + release 资产地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 libssh2-1.11.1，资产文件名与 tag 一致；.tar.bz2 / .tar.xz /
# .zip / .asc 不取。写入前由 final_url 探测兜底。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/libssh2/libssh2.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/(?P<link>libssh2-[0-9][0-9.]*)\n',
    "version": r'^libssh2-(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/libssh2/libssh2/releases/download/{link}/{link}.tar.gz",
}
