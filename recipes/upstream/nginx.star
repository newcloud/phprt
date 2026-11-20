# nginx 的上游查询规则：nginx.org 下载目录索引。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# /download/ 目录索引列出全部历史版本，文件名 nginx-<主>.<次>.<补>.tar.gz；
# 主线奇数版本（1.29.x）和稳定偶数版本（1.28.x）混在一起，靠 catalog 的线
# 归属过滤。线归属和版本比较在 Rust 侧。
UPSTREAM = {
    "url": "https://nginx.org/download/",
    "link": r'href="(?P<link>nginx-[0-9]+\.[0-9]+\.[0-9]+\.tar\.gz)"',
    "version": r'^nginx-(?P<version>[0-9]+\.[0-9]+\.[0-9]+)\.tar\.gz$',
    "join": "https://nginx.org/download/{link}",
}
