TARGET = ext(
    name = "xdebug",
    origin = "pecl",
    version = "3.4.7",
    modules = ["xdebug"],
    link = ["shared"],
    url = "https://pecl.php.net/get/xdebug-3.4.7.tgz",
    ini = "30-xdebug.ini",
    ini_lines = ["zend_extension=xdebug.so"],
    desc = "Xdebug 扩展，契约只支持动态编译",
)
