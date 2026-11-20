# curl 的上游查询规则：curl.se 下载目录索引。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# /download/ 目录索引列出全部历史版本（82 个 .tar.xz），最新包和 7.88.0 这类
# 老线都能命中；下载页 download.html 只列最新，不能用。文件名是
# curl-<主>.<次>.<补>.tar.xz，线归属和版本比较在 Rust 侧。
UPSTREAM = {
    "url": "https://curl.se/download/",
    "link": r'href="/download/(?P<link>curl-[0-9]+\.[0-9]+\.[0-9]+\.tar\.xz)"',
    "version": r'^curl-(?P<version>[0-9]+\.[0-9]+\.[0-9]+)\.tar\.xz$',
    "join": "https://curl.se/download/{link}",
}
