# PHP 所有系列共用的产品 profile。
#
# 版本差异（更高工具要求、特殊 CFLAGS）在系列 recipe 里用 extend_profile 覆盖。

PHP_INI = """[PHP]
date.timezone = Asia/Shanghai
memory_limit = 256M
error_reporting = E_ALL
display_errors = On
log_errors = On
"""

SSL_INI = """; 由 phprt 生成
openssl.cafile = "${SSL_CERT_FILE}"
curl.cainfo    = "${SSL_CERT_FILE}"
"""

PROFILE = product_profile(
    tools = [
        tool("gcc", min = "9"),
        tool("g++", min = "9"),
        tool("make", min = "4.0"),
        "ar",
        "ranlib",
        tool("pkg-config", min = "0.29"),
        tool("patchelf", min = "0.13"),
        tool("perl", min = "5.20"),
        tool("m4", min = "1.4"),
        "sh",
    ],
    modes = {
        "static": mode(strategy = "fold"),
        "shared": mode(
            strategy = "standalone",
            steps = [
                "phpize",
                "./configure --with-php-config={tree}/bin/php-config {flags}",
                "make",
                "make install",
            ],
            artifacts = ["lib/php/extensions/*/{name}.so"],
        ),
    },
    defaults = {"bundled": "static", "external": "shared"},
    bundled_path = "ext/{name}",
    runtime = runtime(
        lib_dir = "lib/phprt",
        rpath = {
            "bin/*": "$ORIGIN/../lib/phprt",
            "lib/php/extensions/*/*.so": "$ORIGIN/../../../phprt",
        },
    ),
    register = {"shared": "php-ini"},
    post_install = [
        ensure_dir("etc/conf.d"),
        write_file("etc/php.ini", PHP_INI, if_missing = True),
        copy_ca_bundle("etc/ssl/cert.pem", [
            "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/pki/tls/certs/ca-bundle.crt",
            "/etc/ssl/cert.pem",
        ]),
        write_file("etc/conf.d/00-ssl.ini", SSL_INI),
        write_component_ini("etc/conf.d", register = "php-ini"),
    ],
    verify = ["{tree}/bin/php", "-v"],
    verify_modules = "{tree}/bin/php -m",
    entry = "myphp-{series}",
    entry_template = "entry.sh",
    env = {
        "CC": "gcc",
        "CXX": "g++",
        "PHP_AUTOCONF": "{tool:autoconf}/bin/autoconf",
        "PHP_AUTOHEADER": "{tool:autoconf}/bin/autoheader",
    },
    audit = "php-config-m4",
)
