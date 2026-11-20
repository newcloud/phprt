# openssl 的上游查询规则：GitHub Releases API。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位，
# 线归属和版本比较在 Rust 侧。一次请求返回全部 Release，openssl 的
# 1.1.1 / 3.0 / 3.5 / 4.0 几条线共享这一次查询。
#
# 3.0 及以后的 asset 文件名是 openssl-<版本>.tar.gz；1.1.1 线的 tag 是
# OpenSSL_1_1_1w，但 asset 文件名仍是 openssl-1.1.1w.tar.gz，所以 version
# 规则照文件名写。限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/openssl/openssl/releases?per_page=100",
    "link": r'"browser_download_url":\s*"(?P<link>[^"]+\.tar\.gz)"',
    "version": r'openssl-(?P<version>[0-9]+(?:\.[0-9]+)*[a-z]?)\.tar\.gz',
}
