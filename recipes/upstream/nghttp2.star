# nghttp2 的上游查询规则：GitHub Releases API。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 一次请求取最新 100 条 Release（全仓 136 条，catalog 的线都在最新
# 100 条内），1.64 等线共享这一次查询。asset 有 .tar.bz2 / .tar.xz /
# .asc / checksums.txt，只取 .tar.gz；tag 形如 v1.64.0，文件名前缀是
# nghttp2-。线归属和版本比较在 Rust 侧。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/nghttp2/nghttp2/releases?per_page=100",
    "link": r'"browser_download_url":\s*"(?P<link>[^"]+\.tar\.gz)"',
    "version": r'nghttp2-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz',
}
