"""phprt recipe 的公共构造函数。

约定：每个 recipe 文件在顶层定义一个全局变量 TARGET：

    TARGET = dep(...) / ext(...) / php(...)
    TARGET = target          # def target(ctx): ...

构造函数只返回 dict，由 Rust 侧校验字段、注入版本并解析依赖。
recipe 里不执行命令、不访问网络、不读写文件。
"""


def feature(needs = [], configure = [], disable = [], requires = {}):
    """一个可选 feature。

    dep 的 feature：由 ext 的 dep:NAME[feature] 打开，needs 是它额外需要的库，
    disable 是 feature 未启用时应用的参数，用来显式关掉上游的自动探测。

    ext 的 feature：由 --ext NAME=feature:[...] 打开，是扩展自己的编译开关。
    ext 的 feature 也可以写 needs：只有选中它时才引入这个依赖（如 swoole 的 cares）。

    requires 声明可用条件：

        requires = {
            "php": ["8.5", "8.6"],              # 允许的 PHP 系列（只用于 ext feature）
            "dep": {"openssl": ["3.5", "4.0"]},  # 允许的依赖线（只用于 ext feature）
            "commands": ["nasm"],               # 需要出现在 PATH 里的命令
        }

    显式选择不满足时报错；bundle 默认不满足时提示并丢弃。
    """
    return {
        "needs": needs,
        "configure": configure,
        "disable": disable,
        "requires": requires,
    }


def dep(
        name,
        version = "",
        url = "",
        sha256 = "",
        needs = [],
        features = {},
        ignored = {},
        configure = [],
        make = ["make"],
        artifacts = [],
        desc = ""):
    """一个需要独立编译并安装到 deps/<name>-<version>/ 的 C 库。

    版本、下载地址由 catalog 和 ctx 注入；recipe 只写构建参数。
    """
    return {
        "kind": "dep",
        "name": name,
        "version": version,
        "url": url,
        "sha256": sha256,
        "needs": needs,
        "features": features,
        "ignored": ignored,
        "configure": configure,
        "make": make,
        "artifacts": artifacts,
        "desc": desc,
    }


def ext(
        name,
        modules,
        link,
        version = "",
        needs = [],
        tools = [],
        dep_configure = {},
        features = {},
        ignored = {},
        configure = [],
        shared_from = "",
        url = "",
        sha256 = "",
        ini = "",
        ini_lines = [],
        artifacts = [],
        desc = ""):
    """一个 PHP 扩展。

    是否内置、是否默认启用都不在这里：内置列表和关闭参数在 php<系列>.star，
    默认启用集合在 bundles/php<系列>.toml。

    link 是契约：只允许这里列出的链接方式。内置扩展默认 static，
    非内置扩展默认 shared；两者都由 Php 系列的内置列表推导。

    static：configure 参数交给 PHP 的 ./configure，编进 PHP 本体，不写 ini。
    shared：内置扩展从 PHP 源码的 shared_from 拷出来，PECL 用 url 下载源码；
            然后 phpize 编成 .so，写 etc/conf.d/<ini> 让 PHP 启动时加载。

    tools 是构建期工具依赖（如 autoconf）：参与构建顺序和 PATH，不参与链接。

    dep_configure 按依赖名声明「这个依赖可以集成」：出现即表示允许
    dep:NAME 选择它；值是选中时额外加给扩展的 configure 参数（可以为空）。
    它的 key 必须是 needs 里的依赖。

    features 是扩展自己的开关：名字 -> feature(configure=[])。
    ignored 是「看过但决定不暴露」的上游选项：选项 -> 理由。
    两项都由 phprt audit 对照上游选项清单检查，避免漏掉或静默忽略。
    """
    return {
        "kind": "ext",
        "name": name,
        "version": version,
        "modules": modules,
        "link": link,
        "needs": needs,
        "tools": tools,
        "dep_configure": dep_configure,
        "features": features,
        "ignored": ignored,
        "configure": configure,
        "shared_from": shared_from,
        "url": url,
        "sha256": sha256,
        "ini": ini,
        "ini_lines": ini_lines,
        "artifacts": artifacts,
        "desc": desc,
    }


def php(
        series,
        version = "",
        url = "",
        sha256 = "",
        builtin_exts = [],
        builtin_disable = {},
        configure = [],
        make = ["make", "make install"],
        artifacts = ["bin/php"],
        desc = ""):
    """PHP 本体。

    series 是 8.5 这样的系列，补丁和下载地址由 catalog 注入。
    builtin_exts / builtin_disable 是 PHP 版本事实：哪些扩展是内置的、
    没选中某个内置扩展时用哪个 configure 参数关闭它。
    默认启用集合不在这里，写在 bundles/php<系列>.toml 的 default_exts。
    """
    return {
        "kind": "php",
        "series": series,
        "name": series,
        "version": version,
        "url": url,
        "sha256": sha256,
        "builtin_exts": builtin_exts,
        "builtin_disable": builtin_disable,
        "configure": configure,
        "make": make,
        "artifacts": artifacts,
        "desc": desc,
    }
