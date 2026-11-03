# downloads/ —— 预下载的源码包

这里放编译要用的 tarball，`install.sh` 会硬链接（跨盘时复制）到安装根的
`downloads/`，于是**完全不用联网**。文件名必须和下面的清单一致（= URL 最后一段）。

这些文件不进 git（`.gitignore` 里排除了），只保留本 README 和 `SHA256SUMS`。

## 需要的文件（PHP 8.5.12 + 常用档）

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
| `redis-6.3.0.tgz` | 390K | https://pecl.php.net/get/redis-6.3.0.tgz |
| `autoconf-2.72.tar.xz` | 1.4M | https://ftp.gnu.org/gnu/autoconf/autoconf-2.72.tar.xz |
| `perl-5.42.2.tar.xz` | 14M | https://www.cpan.org/src/5.0/perl-5.42.2.tar.xz |
| `composer-latest-stable.phar` | 3.7M | https://getcomposer.org/download/latest-stable/composer.phar |

合计约 96M。少几个也能装，只是缺的那个会联网下载。

## 分发给别人

```bash
# 打包（从安装根导出一份，或直接打包仓库的 downloads/）
tar czf phprt-pkg-2026-11.tar.gz -C ~/.phprt downloads

# 对方：解到 phprt 仓库的 downloads/ 再装，全程不联网
tar xzf phprt-pkg-2026-11.tar.gz -C ~/src/phprt/
cd ~/myproject && ~/src/phprt/install.sh --offline
```

## 注意

- `SHA256SUMS` 是校验清单。文件放进来的第一次会按它校验；清单里没有的会在首次
  使用后追加（TOFU）。想严格校验就在 `config.conf` 里填 `*_SHA256=`。
- `composer-latest-stable.phar` 文件名固定，**只要在就不会重新下载**。想换新版：
  `rm ~/.phprt/downloads/composer-latest-stable.phar` 再 `phprt build --only composer`。
- 改 `config.conf` 里的依赖版本后，记得把对应版本的包也放进这里，否则会去联网。
