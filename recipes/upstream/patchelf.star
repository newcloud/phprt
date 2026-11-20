# patchelf 的上游查询规则：GitHub Releases API。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 一次请求取最新 100 条 Release，0.19 等线共享这一次查询。每平台一个
# tar.gz（patchelf-0.19.3-aarch64.tar.gz 等），version 正则要求版本号后
# 紧跟 .tar.gz，平台变体不进候选；还有 .tar.bz2 和 .exe。线归属和版本
# 比较在 Rust 侧。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/NixOS/patchelf/releases?per_page=100",
    "link": r'"browser_download_url":\s*"(?P<link>[^"]+\.tar\.gz)"',
    "version": r'patchelf-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz',
}
