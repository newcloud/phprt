TARGET = php(
    version = "8.5.12",
    url = "https://www.php.net/distributions/php-8.5.12.tar.gz",
    exts = ["openssl", "curl", "swoole", "xdebug"],
    configure = [
        "--prefix={tree}",
        "--with-config-file-path={tree}/etc",
        "--with-config-file-scan-dir={tree}/etc/conf.d",
        "--disable-cgi",
    ],
    make = ["make -j8", "make install"],
    artifacts = ["bin/php"],
    desc = "PHP 8.5 本体",
)
