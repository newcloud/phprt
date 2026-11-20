# libpng 的上游查询规则：SourceForge 的 RSS 文件列表。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# SourceForge 的文件页是 JS 应用，拿不到稳定 HTML；RSS 里每条文件是
#   <link>https://sourceforge.net/projects/libpng/files/libpng16/1.6.59/libpng-1.6.59.tar.xz/download</link>
# 取相对路径，拼成 catalog 用的 downloads.sourceforge.net 直链（不落 /download
# 跳转和镜像临时地址）。1.6.x 目录里还有 lpng1659.zip / 7z，版本正则只认
# libpng-<版本>.tar.xz。
UPSTREAM = {
    "url": "https://sourceforge.net/projects/libpng/rss?path=/libpng16",
    "link": r'<link>https://sourceforge.net/projects/libpng/files/(?P<link>libpng16/[^<]*/libpng-[0-9.]+\.tar\.xz)/download</link>',
    "version": r'^libpng16/(?P<version>[0-9]+\.[0-9]+\.[0-9]+)/libpng-[0-9.]+\.tar\.xz$',
    "join": "https://downloads.sourceforge.net/project/libpng/{link}",
}
