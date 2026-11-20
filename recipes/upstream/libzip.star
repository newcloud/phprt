# libzip 的上游查询规则：libzip.org 下载页。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# 下载页对同一个文件同时给绝对（/download/libzip-<版本>.tar.gz）和相对
# （libzip-<版本>.tar.gz）两种 href，link 正则两种都接；版本正则带首尾锚点，
# 只认 .tar.gz（同页还有 .tar.xz）。catalog 存 libzip.org 的稳定地址。
UPSTREAM = {
    "url": "https://libzip.org/download/",
    "link": r'href="(?:/download/)?(?P<link>libzip-[^"]+\.tar\.gz)"',
    "version": r'^libzip-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz$',
    "join": "https://libzip.org/download/{link}",
}
