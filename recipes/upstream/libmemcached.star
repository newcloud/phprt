# libmemcached（awesomized 维护的分支）的上游查询规则：GitHub tags API +
# tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 不带 v 前缀（1.1.4），下载地址按 archive/refs/tags/<tag>.tar.gz 拼。
# 仓库里还有 `_23` / `pre_cmake` 这类非版本 tag，version 正则带首尾锚点把它们
# 排除；1.2.x 是 cmake 构建，不属于 catalog 的 1.1 线，由线归属过滤。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/awesomized/libmemcached/tags?per_page=100",
    "link": r'"name":\s*"(?P<link>[0-9][^"]*)"',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)+)$',
    "join": "https://github.com/awesomized/libmemcached/archive/refs/tags/{link}.tar.gz",
}
