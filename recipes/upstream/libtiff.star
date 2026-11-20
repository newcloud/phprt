# libtiff 的上游查询规则：osgeo.org 下载目录索引。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# /libtiff/ 目录索引列出全部版本，文件名 tiff-<主>.<次>.<补>.tar.gz；同一
# 目录里还有 tiff-4.7.2rc4.tar.gz 这类候选包，版本正则带首尾锚点只认正式
# 版本（rc 包在版本号和 .tar.gz 之间多了 rcN，不匹配）。
UPSTREAM = {
    "url": "https://download.osgeo.org/libtiff/",
    "link": r'href="(?P<link>tiff-[^"]+\.tar\.gz)"',
    "version": r'^tiff-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz$',
    "join": "https://download.osgeo.org/libtiff/{link}",
}
