IMAGE_LINES = {
    "zlib": ["1.3"],
    "libpng": ["1.6"],
    "jpeg": ["3.1"],
    "webp": ["1.6"],
    "aom": ["3.12"],
    "avif": ["1.3"],
    "freetype": ["2.13"],
}

# gd 没有同名依赖，不能写 PHP_LINES；这里按 PHP 系列列出每个图片依赖允许的线。
# 新增 PHP 系列时必须补一条，否则加载 recipe 会失败。
PHP_DEP_LINES = {
    series: IMAGE_LINES
    for series in ["7.4", "8.0", "8.2", "8.3", "8.4", "8.5", "8.6"]
}


def target(ctx):
    builtin = ctx["is_builtin"]
    self = ctx["self"]

    return ext(
        name = "gd",
        modules = ["gd"],
        link = ["static", "shared"],
        needs = ["libpng"],
        tools = ["autoconf"],
        # jpeg 由 format feature 条件引入；这里只打开依赖 feature 的转发。
        dep_configure = {"jpeg": []},
        features = {
            "jpeg": feature(
                needs = ["jpeg"],
                configure = ["--with-jpeg"],
            ),
            "webp": feature(
                needs = ["webp"],
                configure = ["--with-webp"],
            ),
            "freetype": feature(
                needs = ["freetype"],
                configure = ["--with-freetype"],
            ),
            "avif": feature(
                needs = ["aom", "avif"],
                configure = ["--with-avif"],
                requires = {"php": ["8.2", "8.3", "8.4", "8.5", "8.6"]},
            ),
        },
        ignored = {
            "--with-external-gd": "少用：改用系统 libgd，版本和功能不可控",
            "--with-xpm": "少用：XPM 是旧 X11 图标格式，需要额外 libXpm",
            "--enable-gd-jis-conv": "少用：只影响 JIS 日文字体映射",
        },
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        configure = ["--enable-gd"],
        shared_from = "ext/gd",
        ini = "20-gd.ini",
        desc = "PHP gd 扩展；PNG 固定由 libpng 提供，JPEG/WebP/AVIF/FreeType 可选",
    )


TARGET = target
