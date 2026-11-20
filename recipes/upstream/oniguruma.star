# oniguruma 的上游查询规则：GitHub Releases API。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 一次请求取最新 100 条 Release，6.9 等线共享这一次查询。文件名前缀是
# onig-（不是 oniguruma-）；tag 形如 v6.9.10。还有 .sha256 sidecar，
# 不进候选。线归属和版本比较在 Rust 侧。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/kkos/oniguruma/releases?per_page=100",
    "link": r'"browser_download_url":\s*"(?P<link>[^"]+\.tar\.gz)"',
    "version": r'onig-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz',
}
