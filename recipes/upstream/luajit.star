# luajit（openresty/luajit2）的上游查询规则：GitHub tags API + tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# luajit2 只按日期打快照 tag：v2.1-20261117；下载地址按
# archive/refs/tags/<tag>.tar.gz 拼。link / version 正则只认 2.1-<数字> 这种
# 形式，v2.1.0-beta3 这类旧 tag 不进候选；版本用完整快照号（2.1-20261117），
# 连字符后缀按数值比较，线归属见 docs/CATALOG-UPDATE.md 6.2。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/openresty/luajit2/tags?per_page=100",
    "link": r'"name":\s*"(?P<link>v2\.1-[0-9]+)"',
    "version": r'^v(?P<version>2\.1-[0-9]+)$',
    "join": "https://github.com/openresty/luajit2/archive/refs/tags/{link}.tar.gz",
}
