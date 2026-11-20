# pcre2 的上游查询规则：GitHub Releases API。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 一次请求取最新 100 条 Release，10.49 等线共享这一次查询。asset 有
# .tar.bz2 / .zip / .sig，只取 .tar.gz；tag 形如 pcre2-10.49，文件名
# 前缀与 tag 一致。线归属和版本比较在 Rust 侧。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/PCRE2Project/pcre2/releases?per_page=100",
    "link": r'"browser_download_url":\s*"(?P<link>[^"]+\.tar\.gz)"',
    "version": r'pcre2-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz',
}
