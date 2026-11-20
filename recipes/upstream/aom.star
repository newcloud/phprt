# aom 的上游查询规则：GCS S3 XML 列表。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# 列表未分页（IsTruncated=false），一次请求返回全部 Key；Key 形如
# libaom-<版本>.tar.gz，.asc 签名文件不进 link 正则。线归属和版本比较在
# Rust 侧。
UPSTREAM = {
    "url": "https://storage.googleapis.com/aom-releases/",
    "link": r'<Key>(?P<link>libaom-[0-9.]+\.tar\.gz)</Key>',
    "version": r'libaom-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz',
    "join": "https://storage.googleapis.com/aom-releases/{link}",
}
