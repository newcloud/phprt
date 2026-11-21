# jpeg（libjpeg-turbo）的上游查询规则：git info/refs 列 tag + release 资产地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 是纯版本号（3.1.4.1）；发布页还有 .exe / .rpm / .deb / .dmg，
# 源码包文件名前缀是 libjpeg-turbo-。3.1.90 这类 beta 版本号靠 Rust 侧
# is_unstable 拦。写入前由 final_url 探测兜底。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/libjpeg-turbo/libjpeg-turbo.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/(?P<link>[0-9][0-9.]*)\n',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/{link}/libjpeg-turbo-{link}.tar.gz",
}
