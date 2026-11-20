# libyaml 的上游查询规则：pyyaml.org 下载目录索引。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 索引里同版本有 .tar.gz 和 .zip，只取 .tar.gz（catalog 用的扩展名）。
# 线归属和版本比较在 Rust 侧。
UPSTREAM = {
    "url": "https://pyyaml.org/download/libyaml/",
    "link": r'href="(?P<link>yaml-[0-9.]+\.tar\.gz)"',
    "version": r'yaml-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz',
    "join": "https://pyyaml.org/download/libyaml/{link}",
}
