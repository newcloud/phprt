# libxml2 的上游查询规则：GNOME 目录索引。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# GNOME 按大版本分目录（/sources/libxml2/<线>/），URL 和 join 都带 {line}，
# 这是源形态强制的按线访问；索引里混着 .news / .sha256sum / LATEST-IS-*
# 和 ../，link 只认 .tar.xz。线归属和版本比较在 Rust 侧。
UPSTREAM = {
    "url": "https://download.gnome.org/sources/libxml2/{line}/",
    "link": r'href="(?P<link>libxml2-[0-9.]+\.tar\.xz)"',
    "version": r'libxml2-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.xz',
    "join": "https://download.gnome.org/sources/libxml2/{line}/{link}",
}
