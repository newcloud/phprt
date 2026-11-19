# 构建目标：PHP 的 SAPI。每个版本单独声明，版本差异直接写在这里。
#
# 已核对 7.4～8.5 的 configure：
#   --disable-cli、--disable-cgi 存在；--enable-fpm 存在；
#   --disable-fpm 不存在（FPM 默认关闭，未选中时不生成禁用参数）。
# 8.6 源码暂未取到；SAPI 选项先按 8.5 的同名参数声明，升级时再核对。
TARGETS = {
    "cli": {
        "flags": ["--enable-cli"],
        "disable_flags": ["--disable-cli"],
        "artifacts": ["bin/php"],
        "verify": ["{tree}/bin/php", "-v"],
        "verify_modules": "{tree}/bin/php -m",
        "entry": "myphp-{series}",
        "entry_template": "entry.sh",
    },
    "fpm": {
        "flags": ["--enable-fpm"],
        "disable_flags": [],
        "artifacts": ["sbin/php-fpm"],
        "verify": ["{tree}/sbin/php-fpm", "-v"],
        "config": {
            "user": {"type": "string", "flags": "--with-fpm-user={value}"},
            "group": {"type": "string", "flags": "--with-fpm-group={value}"},
        },
    },
    "cgi": {
        "flags": ["--enable-cgi"],
        "disable_flags": ["--disable-cgi"],
        "artifacts": ["bin/php-cgi"],
        "verify": ["{tree}/bin/php-cgi", "-v"],
    },
}

TOOLCHAIN = toolchain(
    gcc = [toolchain_range(min = "9")],
    gxx = [toolchain_range(min = "9")],
)

TARGET = product(
    name = "php",
    toolchain = TOOLCHAIN,
    profile = PROFILE,
    series = "8.6",
    # 该系列还没完整适配：先只声明当前有 recipe 的内置扩展，
    # 其余内置事实等适配该系列时再补全。
    bundled = ["openssl", "curl", "libxml", "zlib"],
    bundled_disable = {
        "openssl": "--without-openssl",
        "curl": "--without-curl",
        "libxml": "--without-libxml",
        "zlib": "--without-zlib",
    },
    configure = [
        "--prefix={tree}",
        "--with-config-file-path={tree}/etc",
        "--with-config-file-scan-dir={tree}/etc/conf.d",
        "--without-pear",
        "--enable-filter",
        "--enable-phar",
        "--enable-fileinfo",
        "--enable-bcmath",
        "--enable-calendar",
        "--enable-exif",
        # 未适配的内置扩展显式关闭，避免探测系统库。
        # 内置扩展的正向参数由 ext recipe 的 configure 提供；
        # 关闭参数在各系列 php recipe 的 bundled_disable（未适配系列暂时写在 configure 里）。
        "--without-sqlite3",
        "--without-pdo-sqlite",
        "--disable-dom",
        "--disable-simplexml",
        "--disable-xml",
        "--disable-xmlreader",
        "--disable-xmlwriter",
    ],
    make = ["make", "make install"],
    artifacts = [],
    targets = TARGETS,
    desc = "PHP 8.6 本体",
)
