# autoconf 的上游查询规则：GNU FTP 目录索引。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 索引里有 autoconf-2.7-2.8.diff.gz 这种名字里带连字符的文件，link 只认
# <数字.数字>.tar.xz，diff 和 .sig 都进不了候选。线归属和版本比较在 Rust 侧。
UPSTREAM = {
    "url": "https://ftp.gnu.org/gnu/autoconf/",
    "link": r'href="(?P<link>autoconf-[0-9.]+\.tar\.xz)"',
    "version": r'autoconf-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.xz',
    "join": "https://ftp.gnu.org/gnu/autoconf/{link}",
}
