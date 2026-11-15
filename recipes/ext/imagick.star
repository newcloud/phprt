LINES = {
    "3.8": {"php": ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"], "imagemagick": ["7.1"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_builtin"]

    return ext(
        name = "imagick",
        modules = ["imagick"],
        link = ["shared"],
        needs = ["imagemagick"],
        tools = ["autoconf"],
        # ImageMagick 关掉了 OpenMP，这里也不链接 libgomp；把上游的
        # omp_pause_resource_all 探测固定为 no，避免链接不存在的符号。
        configure = [
            "ac_cv_func_omp_pause_resource_all=no",
            "--with-imagick={dep:imagemagick}",
        ],
        version = ctx["php_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "20-imagick.ini",
        desc = "ImageMagick 绑定扩展，依赖树内 ImageMagick",
    )


TARGET = target
