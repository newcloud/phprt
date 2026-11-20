# jpeg（libjpeg-turbo）的上游查询规则：GitHub Releases API。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 一次请求取最新 100 条 Release，3.1 等线共享这一次查询。asset 主要是
# .exe / .rpm / .deb / .dmg，只 .tar.gz 进候选；tag 是纯版本号
# （3.1.4.1），文件名前缀是 libjpeg-turbo-。3.1.90 这类 beta 版本号靠
# Rust 侧 is_unstable 拦。线归属和版本比较在 Rust 侧。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/libjpeg-turbo/libjpeg-turbo/releases?per_page=100",
    "link": r'"browser_download_url":\s*"(?P<link>[^"]+\.tar\.gz)"',
    "version": r'libjpeg-turbo-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz',
}
