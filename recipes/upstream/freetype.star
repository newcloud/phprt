# freetype 的上游查询规则：savannah 目录索引。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 每个版本同时给 .tar.gz 和 .tar.xz（外加 .sig）；catalog 用 .tar.xz，
# 这里固定取 .tar.xz。线归属和版本比较在 Rust 侧。
UPSTREAM = {
    "url": "https://download.savannah.gnu.org/releases/freetype/",
    "link": r'href="(?P<link>freetype-[0-9.]+\.tar\.xz)"',
    "version": r'freetype-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.xz',
    "join": "https://download.savannah.gnu.org/releases/freetype/{link}",
}
