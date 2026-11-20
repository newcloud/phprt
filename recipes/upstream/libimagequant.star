# libimagequant 的上游查询规则：GitHub tags API + tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 不带 v 前缀（2.18.0），下载地址按 archive/refs/tags/<tag>.tar.gz 拼。
# 4.x 的 tag 也在列表里，但不属于 catalog 的 2.18 线，由 Rust 侧线归属过滤。
# version 正则带首尾锚点，rc 这类后缀的 tag 不进候选。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/ImageOptim/libimagequant/tags?per_page=100",
    "link": r'"name":\s*"(?P<link>[0-9][^"]*)"',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)+)$',
    "join": "https://github.com/ImageOptim/libimagequant/archive/refs/tags/{link}.tar.gz",
}
