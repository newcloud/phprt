TARGET = php(
    series = "8.2",
    # 该系列还没完整适配：先只声明当前有 recipe 的内置扩展，
    # 其余内置事实等适配该系列时再补全。
    builtin_exts = ["openssl", "curl", "libxml"],
    builtin_disable = {
        "openssl": "--without-openssl",
        "curl": "--without-curl",
        "libxml": "--without-libxml",
    },
    configure = [
        "--prefix={tree}",
        "--with-config-file-path={tree}/etc",
        "--with-config-file-scan-dir={tree}/etc/conf.d",
        "--enable-cli",
        "--disable-cgi",
        "--without-pear",
        "--enable-filter",
        "--enable-phar",
        "--enable-fileinfo",
        "--enable-bcmath",
        "--enable-calendar",
        "--enable-exif",
        # v3 还没有 zlib / sqlite 的 recipe；这些系列也没默认启用 libxml2，
        # 显式关闭，避免探测系统库。
        # 内置扩展的正向参数由 ext recipe 的 configure 提供；
        # 关闭参数在各系列 php recipe 的 builtin_disable（未适配系列暂时写在 configure 里）。
        "--without-zlib",
        "--without-sqlite3",
        "--without-pdo-sqlite",
        "--disable-dom",
        "--disable-simplexml",
        "--disable-xml",
        "--disable-xmlreader",
        "--disable-xmlwriter",
    ],
    make = ["make", "make install"],
    artifacts = ["bin/php"],
    desc = "PHP 8.2 本体",
)
