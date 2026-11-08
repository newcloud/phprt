# 扩展覆盖计划（PHP 8.5 为主）

## 背景

phprt php8.5 目前 `SUPPORTED_EXTS` 17 个（builtin 11 + PECL 6），框架必需扩展已全覆盖，
但对比主流环境缺了不少常用扩展。判断依据：

- Clever Cloud PHP 8.2–8.5 默认开启清单（含 sodium/gettext/xsl/ldap/gmp/pdo_pgsql/imagick/memcached/mongodb/amqp 等）
- StaticPHP 支持列表（static-php.dev）
- MediaWiki / Symfony / Magento 官方要求页（Magento 硬性要求 ext-xsl）
- Phil's Blog《List of commonly used PHP Extensions》(2025)

## 分类与约定

- builtin = `bundles/php8.5/builtin/<名>.sh`，由 PHP 本体 configure 编译；
  PECL = `bundles/php8.5/ext/<名>.sh`，phpize 独立构建；依赖库 = `deps/<库>.sh`。
  流程见 docs/ADD-EXTENSION.md。
- 每个 builtin 的开关名由配方头部 `#  phprt: builtin=<名> switch=ENABLE_<名>` 认领；
  新依赖库的版本变量写 bundle.conf（仓库是唯一版本源）。
- 加 builtin 要同步两处 `BUILTIN_ORDER`：`runner/build.sh` 和 `bundles/php8.5/php.sh`。
- 本轮只改 php8.5；php8.6 / php7.4 以后照抄。

## 升级判断（通用规则）

- **builtin 扩展**：configure 行变化 → PHP 签名变 → PHP 重编一次；已装 PECL 扩展的
  `ext_sig` 不含 configure 行、php_abi 不变 → 不触发重编；新依赖库只进 `deps/` 并被
  wheelize 成 rt 名，无消费方引用 → 消费方 DT_NEEDED 零改写、零重编。
  老树用 `phprt add ext <名>` 挂载（builtin 会提示重编 PHP）。
- **PECL 扩展**：`ext/<名>.sh` 独立，`phprt add ext <名>` 直接装，不动 PHP。
- **新依赖库**：只新增 `deps/<库>-<版本>/`，不影响现有库。
- 现有项目（blog）每加一个都实测：构建 + `php -m` + 功能调用 + `phprt verify` + 搬移测试。

## 待加清单与进度

| # | 扩展 | 类型 | 依赖库 | configure 参数（查文档后填） | 状态 |
|---|------|------|--------|------------------------------|------|
| 1 | sodium | builtin | libsodium | `--with-sodium=<deps/libsodium>`（php.net/manual/en/sodium.installation.php） | ✅ 2026-11-08（构建 + php -m + 功能调用 + 上游 .phpt 31/31 + verify + 搬移测试全过） |
| 2 | gettext | builtin | 无（glibc） | `--with-gettext`（php.net/manual/en/gettext.installation.php） | ✅ 2026-11-08（构建 + php -m + 上游 .phpt 19/19 + verify 全过） |
| 3 | xsl | builtin | libxslt | `--with-xsl=<deps/libxslt>`（php.net/manual/en/xsl.installation.php） | ✅ 2026-11-08（构建 + php -m + 上游 .phpt 83 passed/2 skipped/0 failed + 功能调用 + verify + 搬移测试全过） |
| 4 | ftp | builtin | 无 | 待查 | ⬜ |
| 5 | bz2 | builtin | bzip2 | 待查 | ⬜ |
| 6 | igbinary | PECL | 无 | 待查 | ⬜ |
| 7 | msgpack | PECL | 无 | 待查 | ⬜ |
| 8 | ldap | builtin | OpenLDAP | 待查 | ⬜ |
| 9 | gmp | builtin | gmp | 待查 | ⬜ |
| 10 | memcached | PECL | libmemcached | 待查 | ⬜ |
| 11 | pgsql + pdo_pgsql | builtin | libpq | 待查 | ⬜ |
| 12 | tidy | builtin | libtidy | 待查 | ⬜ |
| 13 | mongodb | PECL | libmongoc/libbson | 待查 | ⬜ |
| 14 | amqp | PECL | rabbitmq-c | 待查 | ⬜ |
| 15 | imagick | PECL | ImageMagick 7 | 待查 | ⬜ |
| 16 | ssh2 | PECL | libssh2 | 待查 | ⬜ |
| 17 | rdkafka | PECL | librdkafka | 待查 | ⬜ |

## 明确不做

- imap（uw-imap c-client 构建维护成本高）、grpc（构建链过重）
- mcrypt（已废弃）、odbc / snmp / pspell / dba / solr / oauth（小众）
- readline（php -a 交互非刚需）
- sqlsrv / oci8（私有驱动）

## 实施规则

1. 先写本文档，再逐个实现。
2. 每个扩展先查上游官方文档定 configure 参数，再写配方；参数和文档链接记入上表。
3. 完成一个测试一个（构建 + `php -m` + 功能调用 + `phprt verify` + 搬移测试）。
4. 每完成一个更新上表进度并提交（不 push）。

## 与 docs/RECIPE.md 的关系（v2 迁移债）

docs/RECIPE.md（2026-11-08）要求新 recipe 用 `contract=v2`（头字段矩阵 +
flags()/needs()/build() + 末行 `recipe_main "$@"`），但 driver 还没落地：
common.sh 里没有 `recipe_main`，也没有 `legacy-recipe.list`，现有 110 个
recipe 全是 legacy。本轮新增配方（sodium/gettext/xsl 等）先按 legacy 写
（与现有 recipe 一致、能构建），等 Phase A/B driver 落地后统一迁移 v2，
迁移时同步把本表参数列核对一遍。
