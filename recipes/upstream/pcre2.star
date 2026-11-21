# pcre2 的上游查询规则：git info/refs 列 tag + release 资产地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 pcre2-10.49（-RC1 这类后缀不是纯数字，不进候选）；资产文件名与
# tag 一致，地址按 releases/download/<tag>/<tag>.tar.gz 拼。只有真要写入新
# 补丁时才用这个地址，写入前由 final_url 探测兜底。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/PCRE2Project/pcre2.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/(?P<link>pcre2-[0-9][0-9.]*)\n',
    "version": r'^pcre2-(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/PCRE2Project/pcre2/releases/download/{link}/{link}.tar.gz",
}
