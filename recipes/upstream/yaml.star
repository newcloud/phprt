# yaml 的上游查询规则：PECL REST allreleases XML。
#
# 契约见 docs/CATALOG-UPDATE.md 4.1：url / link / version 三个槽位。
# 接口不给下载直链，只列版本和状态；这里只抽 <s>stable</s> 的版本，下载地址
# 按 PECL 模板拼成 https://pecl.php.net/get/yaml-<版本>.tgz。线归属和版本
# 比较在 Rust 侧。
UPSTREAM = {
    "url": "https://pecl.php.net/rest/r/yaml/allreleases.xml",
    "link": r'<r><v>(?P<link>[^<]+)</v><s>stable</s></r>',
    "version": r'(?P<version>[0-9]+(?:\.[0-9]+)*[a-zA-Z]*[0-9]*)',
    "join": "https://pecl.php.net/get/yaml-{link}.tgz",
}
