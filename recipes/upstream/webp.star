# webp 的上游查询规则：GitHub tags API + GCS 下载地址模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# webp 的 GCS 目录（.../releases/webp/）是 2014 年的空对象，没有列表页，
# 改走 GitHub tags API 拿版本号，下载地址按 GCS 模板拼（与 PECL 同构）。
# rc 后缀的 tag（v1.6.0-rc1）不进 link 正则；线归属和版本比较在 Rust 侧。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/webmproject/libwebp/tags?per_page=100",
    "link": r'"name":\s*"v(?P<link>[0-9]+(?:\.[0-9]+)*)"',
    "version": r'(?P<version>[0-9]+(?:\.[0-9]+)*)',
    "join": "https://storage.googleapis.com/downloads.webmproject.org/releases/webp/libwebp-{link}.tar.gz",
}
