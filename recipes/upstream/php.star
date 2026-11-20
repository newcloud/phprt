# php 的上游查询规则：php.net Releases JSON API。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# php.net 接口按线返回（URL 带 {line}），这是源形态强制的：接口只回答
# 「这条线的最新发布」，一次请求拿不到全部线，所以按线请求。
# source 数组里 tar.gz / tar.bz2 / tar.xz 都有，固定取 .tar.gz（catalog
# 多数线用这个扩展名）；接口自带 sha256，但按 8.1 节统一下载文件自己算。
UPSTREAM = {
    "url": "https://www.php.net/releases/index.php?json&version={line}",
    "link": r'"filename":"(?P<link>php-[0-9.]+\.tar\.gz)"',
    "version": r'php-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz',
    "join": "https://www.php.net/distributions/{link}",
}
