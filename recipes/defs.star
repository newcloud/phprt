"""phprt recipe 的公共构造函数。

约定：每个 recipe 文件在顶层定义一个全局变量 TARGET：

    TARGET = dep(...) / ext(...) / php(...)
    TARGET = target          # def target(ctx): ...

构造函数只返回 dict，由 Rust 侧校验字段、注入版本并解析依赖。
recipe 里不执行命令、不访问网络、不读写文件。
"""


def feature(needs = [], configure = [], disable = []):
    """dep 的一个可选 feature。只有 ext 的 sub 带方括号时才会启用。

    disable 是 feature 未启用时应用的参数，用来显式关掉上游的自动探测。
    """
    return {
        "needs": needs,
        "configure": configure,
        "disable": disable,
    }


def sub(dep = "", configure = []):
    """ext 自己的一个子配置。

    dep：方括号里的 feature 转给哪个依赖；空表示不转。
    configure：选中这个子配置时追加到 ext 的编译参数。
    """
    return {
        "dep": dep,
        "configure": configure,
    }


def dep(
        name,
        version = "",
        url = "",
        sha256 = "",
        needs = [],
        features = {},
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
        subs = {},
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
    subs 是扩展自己的子配置表：选中时应用 configure，
    方括号里的 feature 转给 sub.dep 指向的依赖。
    """
    return {
        "kind": "ext",
        "name": name,
        "version": version,
        "modules": modules,
        "link": link,
        "needs": needs,
        "tools": tools,
        "subs": subs,
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
