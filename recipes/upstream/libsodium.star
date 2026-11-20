# libsodium 的上游查询规则：官方 releases 目录索引。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 索引里有 -mingw / -msvc 变体和 .sig / .minisig，link 只认纯版本号的
# .tar.gz（libsodium-1.0.22.tar.gz），带 -mingw 的不会匹配。线归属和版本
# 比较在 Rust 侧。
UPSTREAM = {
    "url": "https://download.libsodium.org/libsodium/releases/",
    "link": r'href="(?P<link>libsodium-[0-9.]+\.tar\.gz)"',
    "version": r'libsodium-(?P<version>[0-9]+(?:\.[0-9]+)*)\.tar\.gz',
    "join": "https://download.libsodium.org/libsodium/releases/{link}",
}
