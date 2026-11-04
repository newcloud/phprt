# downloads/ —— 源码种子缓存

`phprt set` / `add ext` 会从这里取源码包，取到后硬链接（跨盘时复制）到项目的
`.phprt/downloads/`，于是**完全不用联网**。

**这些文件不进 git**（`.gitignore` 里排除了整个目录，只保留本 README 和 `SHA256SUMS`）。

## 查找顺序

1. `<项目>/.phprt/downloads/`
2. `PHPRT_SEED`（可选）
3. `downloads/php-<主>.<次>/`（版本系列目录）
4. `downloads/`
5. `~/.phprt/downloads/`

系列目录用来放某个 PHP 系列专用的旧版依赖，避免和 8.x 混在一起：

```text
downloads/php-7.4/
├── php-7.4.33.tar.xz
└── libxml2-2.9.14.tar.xz
```

文件名必须和 bundle 里 `*_URL` 的最后一段一致。

## 需要的文件（PHP 8.5.12 全扩展）

| 文件 | 大小 | 来源 |
| --- | --- | --- |
| `php-8.5.12.tar.xz` | 14M | https://www.php.net/distributions/php-8.5.12.tar.xz |
| `openssl-3.5.9.tar.gz` | 51M | https://www.openssl.org/source/openssl-3.5.9.tar.gz |
| `zlib-1.3.2.tar.gz` | 1.5M | https://zlib.net/zlib-1.3.2.tar.gz |
| `libxml2-2.15.4.tar.xz` | 3.1M | https://download.gnome.org/sources/libxml2/2.15/libxml2-2.15.4.tar.xz |
| `sqlite-amalgamation-3540100.zip` | 2.9M | https://sqlite.org/2026/sqlite-amalgamation-3540100.zip |
| `curl-8.23.0.tar.xz` | 2.9M | https://curl.se/download/curl-8.23.0.tar.xz |
| `onig-6.9.10.tar.gz` | 957K | https://github.com/kkos/oniguruma/releases/download/v6.9.10/onig-6.9.10.tar.gz |
| `libzip-1.12.1.tar.xz` | 795K | https://libzip.org/download/libzip-1.12.1.tar.xz |
| `icu4c-79.1-sources.tgz` | 27M | https://github.com/unicode-org/icu/releases/download/release-79.1/icu4c-79.1-sources.tgz |
| `libpng-1.6.50.tar.xz` | 1.1M | https://download.sourceforge.net/libpng/libpng-1.6.50.tar.xz |
| `libjpeg-turbo-3.1.2.tar.gz` | 2.4M | https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/3.1.2/libjpeg-turbo-3.1.2.tar.gz |
| `freetype-2.13.3.tar.xz` | 2.5M | https://download.savannah.gnu.org/releases/freetype/freetype-2.13.3.tar.xz |
| `libwebp-1.6.0.tar.gz` | 4.1M | https://storage.googleapis.com/downloads.webmproject.org/releases/webp/libwebp-1.6.0.tar.gz |
| `libaom-3.12.0.tar.gz` | 5.3M | https://storage.googleapis.com/aom-releases/libaom-3.12.0.tar.gz |
| `libavif-1.3.0.tar.gz` | 14M | https://github.com/AOMediaCodec/libavif/archive/refs/tags/v1.3.0.tar.gz |
| `libffi-3.4.8.tar.gz` | 1.4M | https://github.com/libffi/libffi/releases/download/v3.4.8/libffi-3.4.8.tar.gz |
| `yaml-0.2.5.tar.gz` | 596K | https://pyyaml.org/download/libyaml/yaml-0.2.5.tar.gz |
| `redis-6.3.0.tgz` | 390K | https://pecl.php.net/get/redis-6.3.0.tgz |
| `protobuf-5.36.2.tgz` | 534K | https://pecl.php.net/get/protobuf-5.36.2.tgz |
| `xdebug-3.5.3.tgz` | 278K | https://pecl.php.net/get/xdebug-3.5.3.tgz |
| `apcu-5.1.28.tgz` | 92K | https://pecl.php.net/get/apcu-5.1.28.tgz |
| `yaml-2.3.0.tgz` | 41K | https://pecl.php.net/get/yaml-2.3.0.tgz |
| `swoole-6.2.4.tgz` | 2.6M | https://pecl.php.net/get/swoole-6.2.4.tgz |
| `autoconf-2.72.tar.xz` | 1.4M | https://ftp.gnu.org/gnu/autoconf/autoconf-2.72.tar.xz |
| `perl-5.42.2.tar.xz` | 14M | https://www.cpan.org/src/5.0/perl-5.42.2.tar.xz |
| `re2c-4.6.tar.xz` | 2M | https://github.com/skvadrik/re2c/releases/download/4.6/re2c-4.6.tar.xz |
| `composer-latest-stable.phar` | 3.7M | https://getcomposer.org/download/latest-stable/composer.phar |

PHP 8.6.0-dev 还需要 `PHP-8.6.tar.gz`（GitHub 分支归档）。
PHP 7.4.33 的包放在 `downloads/php-7.4/`。

合计约 180M（含 ICU / libaom / libavif）。少几个也能装，缺的那个会联网下载。

## 分发给别人

```bash
# 打包当前缓存
tar czf phprt-pkg-2026-11.tar.gz -C /path/to/phprt downloads

# 对方：解到 phprt 仓库的 downloads/ 再用，全程不联网
tar xzf phprt-pkg-2026-11.tar.gz -C ~/src/phprt/
```

## 注意

- `SHA256SUMS` 是校验清单。清单里没有的包会在首次使用后追加（TOFU）；
  想严格校验就在 `bundle.conf` 里填 `*_SHA256=`。
- `composer-latest-stable.phar` 文件名固定，**只要在就不会重新下载**。
- 改了 bundle 里的依赖版本，记得把对应版本的包也放进这里，否则会去联网。
