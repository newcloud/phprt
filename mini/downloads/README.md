# mini 源码缓存

`phprt set` 会从这里找源码包；也可以设置 `PHPRT_SEED` 指向已有安装根的 `downloads/`。

查找顺序里包含版本系列目录 `downloads/php-<主>.<次>/`，用来放这一系列专用的旧版依赖，
避免和 8.x 的版本混在一起。例如 PHP 7.4：

```text
downloads/php-7.4/
├── php-7.4.33.tar.xz
├── libxml2-2.9.14.tar.xz
└── ...
```

通用包（zlib、sqlite 等）也可以直接放在本目录或 `~/.phprt/downloads/`。

已准备的包：

- `php-8.5.12.tar.xz`
- `PHP-8.6.tar.gz`
- `php-7.4/php-7.4.33.tar.xz`
- `php-7.4/libxml2-2.9.14.tar.xz`
- `zlib-1.3.2.tar.gz`
- `sqlite-amalgamation-3540100.zip`

本目录里的 tarball 不进 git。
