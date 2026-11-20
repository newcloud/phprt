# librdkafka 的上游查询规则：GitHub tags API + tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 v2.16.0，下载地址按 archive/refs/tags/<tag>.tar.gz 拼。仓库会先打
# v2.16.0-RC4 这类预发布 tag，version 正则带首尾锚点只认纯版本号。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/confluentinc/librdkafka/tags?per_page=100",
    "link": r'"name":\s*"(?P<link>v[0-9][^"]*)"',
    "version": r'^v(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/confluentinc/librdkafka/archive/refs/tags/{link}.tar.gz",
}
