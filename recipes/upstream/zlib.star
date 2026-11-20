# zlib 的上游查询规则：fossils 目录索引。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位，
# 线归属和版本比较在 Rust 侧。连接在页面里是相对地址，用 join 拼成绝对地址。
UPSTREAM = {
    "url": "https://zlib.net/fossils/",
    "link": r'href="(?P<link>zlib-[0-9.]+\.tar\.gz)"',
    "version": r'zlib-(?P<version>[0-9.]+)\.tar\.gz',
    "join": "https://zlib.net/fossils/{link}",
}
