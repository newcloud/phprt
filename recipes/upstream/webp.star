# webp（libwebp）的上游查询规则：git info/refs 列 tag + 官方下载地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# tag 形如 v1.6.0，link 只取版本号；下载地址走 webmproject 的存储桶，
# 文件名是 libwebp-<版本>.tar.gz。线归属和版本比较在 Rust 侧。
# info/refs 是 git 协议入口，不消耗 GitHub API 额度。
UPSTREAM = {
    "url": "https://github.com/webmproject/libwebp.git/info/refs?service=git-upload-pack",
    "link": r'refs/tags/v(?P<link>[0-9][0-9.]*)\n',
    "version": r'^(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://storage.googleapis.com/downloads.webmproject.org/releases/webp/libwebp-{link}.tar.gz",
}
