"""phprt recipe 的公共构造函数。

约定：每个 recipe 文件在顶层定义一个全局变量 TARGET：

    TARGET = dep(...) / ext(...) / php(...)

构造函数只返回 dict，由 Rust 侧校验字段和解析依赖。
recipe 里不执行命令、不访问网络、不读写文件。
"""


def feature(needs = [], configure = []):
    """dep 的一个可选 feature。只有 ext 的 sub 带方括号时才会启用。"""
    return {
        "needs": needs,
        "configure": configure,
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
        version,
        url,
        sha256 = "",
        needs = [],
        features = {},
        configure = [],
        make = ["make"],
        artifacts = [],
        desc = ""):
    """一个需要独立编译并安装到 deps/<name>-<version>/ 的 C 库。

    features 里的子功能由扩展通过 sub: 转发启用；
    configure 里额外的参数和 needs 只在启用时生效。
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
        origin,
        modules,
        link,
        version = "",
        needs = [],
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

    from="builtin" 默认 static，from="pecl" 默认 shared。
    link 是契约：只允许这里列出的链接方式。

    static：configure 参数交给 PHP 的 ./configure，编进 PHP 本体，不写 ini。
    shared：builtin 从 PHP 源码的 shared_from 拷出来，PECL 用 url 下载源码；
            然后 phpize 编成 .so，写 etc/conf.d/<ini> 让 PHP 启动时加载。

    subs 是扩展自己的子配置表：选中时应用 configure，
    方括号里的 feature 转给 sub.dep 指向的依赖。
    """
    return {
        "kind": "ext",
        "name": name,
        "version": version,
        "modules": modules,
        "origin": origin,
        "link": link,
        "needs": needs,
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
        version,
        url,
        exts = [],
        configure = [],
        make = ["make", "make install"],
        artifacts = ["bin/php"],
        desc = ""):
    """PHP 本体。exts 里列出的扩展会参与求解并汇总 configure 参数。"""
    return {
        "kind": "php",
        "name": version,
        "version": version,
        "url": url,
        "exts": exts,
        "configure": configure,
        "make": make,
        "artifacts": artifacts,
        "desc": desc,
    }
