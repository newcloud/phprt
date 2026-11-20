# sqlite 的上游查询规则：官方下载页的机器可读 PRODUCT 行。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# sqlite.org/download.html 末尾有一段给脚本读的清单：
#   PRODUCT,3.54.1,2026/sqlite-autoconf-3540100.tar.gz,3332984,<sha3>
# 文件名里的 3540100 和版本 3.54.1 不是直接对应关系，所以版本从同一行的
# 第 2 列取（link 正则同时给出 version 组，不写 version 槽位）；下载地址是
# 相对路径，按站点模板拼。页面只列当前版本，老线会报「该线没有候选」。
UPSTREAM = {
    "url": "https://www.sqlite.org/download.html",
    "link": r'PRODUCT,(?P<version>[0-9]+\.[0-9]+\.[0-9]+),(?P<link>[^,\n]*sqlite-autoconf-[0-9]+\.tar\.gz),',
    "join": "https://www.sqlite.org/{link}",
}
