# PHP 8.6 的内置扩展事实。
#
# bundled：PHP 8.6 源码 ext/ 下的内置扩展。只从这里推导 ext 的 origin，
#               和当前是否已有 recipe 无关。
# bundled_disable：phprt 需要显式关闭的内置扩展 -> 对应的 configure 关闭参数。
#               选中 static 时会跳过对应的关闭参数，改用 ext recipe 的 configure。
#               不在这里的扩展保持 PHP 默认或 php recipe 里的 --enable-*。
#
# 按 php-8.6.0 的 ext/ 核对：ext/imap 只剩 tests，没有 config.m4，不参与构建；
# 新增的 ext/skeleton 是扩展模板，也不列进来。
BUILTIN_EXTS = [
        "bcmath",
        "bz2",
        "calendar",
        "com_dotnet",
        "ctype",
        "curl",
        "date",
        "dba",
        "dl_test",
        "dom",
        "enchant",
        "exif",
        "ffi",
        "fileinfo",
        "filter",
        "ftp",
        "gd",
        "gettext",
        "gmp",
        "hash",
        "iconv",
        "intl",
        "json",
        "ldap",
        "lexbor",
        "libxml",
        "mbstring",
        "mysqli",
        "mysqlnd",
        "odbc",
        "opcache",
        "openssl",
        "pcntl",
        "pcre",
        "pdo",
        "pdo_dblib",
        "pdo_firebird",
        "pdo_mysql",
        "pdo_odbc",
        "pdo_pgsql",
        "pdo_sqlite",
        "pgsql",
        "phar",
        "posix",
        "random",
        "readline",
        "reflection",
        "session",
        "shmop",
        "simplexml",
        "snmp",
        "soap",
        "sockets",
        "sodium",
        "spl",
        "sqlite3",
        "standard",
        "sysvmsg",
        "sysvsem",
        "sysvshm",
        "tidy",
        "tokenizer",
        "uri",
        "xml",
        "xmlreader",
        "xmlwriter",
        "xsl",
        "zend_test",
        "zip",
        "zlib",
]

# 关闭参数逐个按 php-8.6.0 的 ext/*/config*.m4 与 configure --help 核对：
#   openssl / libxml / sqlite3 / zlib 用 config0.m4，选项名不变；
#   其余扩展的 --enable-*/--with-* 选项名和 8.5 相同。
BUILTIN_DISABLE = {
        "openssl": "--without-openssl",
        "curl": "--without-curl",
        "libxml": "--without-libxml",
        "gd": "--disable-gd",
        "dom": "--disable-dom",
        "simplexml": "--disable-simplexml",
        "xml": "--disable-xml",
        "xmlreader": "--disable-xmlreader",
        "xmlwriter": "--disable-xmlwriter",
        "sqlite3": "--without-sqlite3",
        "pdo_sqlite": "--without-pdo-sqlite",
        "zlib": "--without-zlib",
        "mbstring": "--disable-mbstring",
        "zip": "--without-zip",
        "sodium": "--without-sodium",
        "mysqli": "--without-mysqli",
        "pdo_mysql": "--without-pdo-mysql",
        "gettext": "--without-gettext",
        "pcntl": "--disable-pcntl",
        "sockets": "--disable-sockets",
        "shmop": "--disable-shmop",
        "sysvmsg": "--disable-sysvmsg",
        "sysvsem": "--disable-sysvsem",
        "sysvshm": "--disable-sysvshm",
}

# 构建目标：PHP 的 SAPI。每个版本单独声明，版本差异直接写在这里。
#
# 按 php-8.6.0 核对：--disable-cli 在 sapi/cli/config.m4、--disable-cgi 在
# sapi/cgi/config9.m4、--enable-fpm 在 sapi/fpm/config.m4；--disable-fpm 不存在
# （FPM 默认关闭，未选中时不生成禁用参数）。
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
    bundled = BUILTIN_EXTS,
    bundled_disable = BUILTIN_DISABLE,
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
    ],
    make = ["make", "make install"],
    artifacts = [],
    targets = TARGETS,
    desc = "PHP 8.6 本体",
)
