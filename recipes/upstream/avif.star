# libavif 的上游查询规则：GitHub tags API + tag 归档模板。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version / join 四个槽位。
# libavif 没有 Release asset，只有 tag；tag 形如 v1.4.2，下载地址按
# archive/refs/tags/<tag>.tar.gz 拼（GitHub 会跳到 codeload，catalog 存这个
# 稳定地址）。version 正则带首尾锚点，rc / beta 这类后缀的 tag 不进候选。
# 线归属和版本比较在 Rust 侧。
# 限流：匿名 60 次/小时，设 GITHUB_TOKEN 环境变量可提额。
UPSTREAM = {
    "url": "https://api.github.com/repos/AOMediaCodec/libavif/tags?per_page=100",
    "link": r'"name":\s*"(?P<link>v[0-9][^"]*)"',
    "version": r'^v(?P<version>[0-9]+(?:\.[0-9]+)*)$',
    "join": "https://github.com/AOMediaCodec/libavif/archive/refs/tags/{link}.tar.gz",
}
