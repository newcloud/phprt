# libffi 的上游查询规则：git info/refs 列 tag + release 资产地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 v3.8.2；release 资产名是 libffi-<版本>.tar.gz。
UPSTREAM = {
    "url": "https://github.com/libffi/libffi.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/v(?P<link>[0-9][0-9.]*)\n',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/libffi/libffi/releases/download/v{link}/libffi-{link}.tar.gz",
}
