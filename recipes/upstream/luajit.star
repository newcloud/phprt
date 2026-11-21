# luajit（openresty/luajit2）的上游查询规则：git info/refs 列 tag + tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# luajit2 只按日期打快照 tag：v2.1-20261117；下载地址按
# archive/refs/tags/<tag>.tar.gz 拼。link / version 正则只认 2.1-<数字> 这种
# 形式，v2.1.0-beta3 这类旧 tag 不进候选；版本用完整快照号（2.1-20261117），
# 连字符后缀按数值比较，线归属见 docs/CATALOG-UPDATE.md 6.2。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/openresty/luajit2.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/(?P<link>v2\.1-[0-9]+)\n',
    "version": r'^v(?P<version>2\.1-[0-9]+)$',
    "join": "https://github.com/openresty/luajit2/archive/refs/tags/{link}.tar.gz",
}
