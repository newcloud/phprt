"""phprt recipe 的公共构造函数。

约定：每个 recipe 文件在顶层定义一个全局变量 TARGET：

    TARGET = dep(...) / component(...) / product(...)
    TARGET = target          # def target(ctx): ...

构造函数只返回 dict，由 Rust 侧校验字段、注入版本并解析依赖。
recipe 里不执行命令、不访问网络、不读写文件。
"""


def feature(needs = [], configure = [], disable = [], requires = {}):
    """一个可选 feature。

    dep 的 feature：由组件 spec 的 dep:NAME[feature] 打开，needs 是它额外需要的库，
    disable 是 feature 未启用时应用的参数，用来显式关掉上游的自动探测。

    component 的 feature：由 --component NAME=feature:[...] 打开，是组件自己的编译开关。
    component 的 feature 也可以写 needs：只有选中它时才引入这个依赖（如 swoole 的 cares）。

    requires 声明可用条件：

        requires = {
            "product": ["8.5", "8.6"],          # 允许的产品系列（只用于 component feature）
            "dep": {"openssl": ["3.5", "4.0"]},  # 允许的依赖线（只用于 component feature）
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


def mode(strategy, steps = [], artifacts = [], configure = []):
    """产品的一种组件挂载方式。

    strategy = "fold"：组件只贡献 configure 参数和源码，编进产品本体。
    strategy = "standalone"：产品构建完之后，用 steps 单独构建组件。

    steps / artifacts / configure 都是模板：
        {name} / {product} / {flags} / {build} / {prefix} / {component_source}
    """
    return {
        "strategy": strategy,
        "steps": steps,
        "artifacts": artifacts,
        "configure": configure,
    }


def runtime(lib_dir = "", rpath = {}):
    """安装树的运行时整理：依赖库统一目录 + 相对路径 glob 的 rpath。"""
    return {
        "lib_dir": lib_dir,
        "rpath": rpath,
    }


def product_profile(
        modes = {},
        defaults = {},
        bundled_path = "",
        runtime = {},
        register = {},
        verify = [],
        verify_modules = "",
        entry = "",
        entry_template = "",
        env = {},
        audit = "none",
        config = ""):
    """产品的组件挂载协议、运行时整理、验证和入口。

    modes：挂载方式 -> mode(...)。
    defaults：组件来源（bundled / external）-> 默认挂载方式。
    register：挂载方式 -> 登记动作；目前只有 php-ini。
    verify / verify_modules：产品启动验证；verify_modules 用来检查组件模块。
    entry / entry_template：项目入口文件名模板和内置模板名。
    env：产品专用环境变量，支持 {tree} 和 {tool:<dep>}。
    audit：选项来源，php-config-m4 / configure-help / none。
    config：安装后配置动作，php 或空。
    """
    return {
        "modes": modes,
        "defaults": defaults,
        "bundled_path": bundled_path,
        "runtime": runtime,
        "register": register,
        "verify": verify,
        "verify_modules": verify_modules,
        "entry": entry,
        "entry_template": entry_template,
        "env": env,
        "audit": audit,
        "config": config,
    }


def component(
        name,
        product,
        modes,
        source = "external",
        version = "",
        url = "",
        sha256 = "",
        needs = [],
        tools = [],
        dep_configure = {},
        prepare = [],
        features = {},
        ignored = {},
        configure = [],
        source_path = "",
        modules = [],
        ini = "",
        ini_lines = [],
        env = {},
        artifacts = [],
        desc = ""):
    """一个依附于产品的组件（PHP 扩展、nginx 模块、CPython C 扩展）。

    product 是所属产品，必填；是否内置由产品 recipe 的 bundled 列表决定。
    source = "bundled"：源码在产品源码树里，source_path 是相对路径（如 ext/openssl）。
    source = "external"：组件自己的 url / version，单独下载。

    modes 是契约：只允许这里列出的挂载方式，具体含义由产品 profile 定义。
    默认挂载方式由产品 profile 的 defaults 按 source 推导。

    tools 是构建期工具依赖（如 autoconf）：参与构建顺序和 PATH，不参与链接。

    dep_configure 按依赖名声明「这个依赖可以集成」：出现即表示允许
    dep:NAME 选择它；值是选中时额外加给组件的 configure 参数（可以为空）。
    它的 key 必须是 needs 里的依赖。

    features 是组件自己的开关：名字 -> feature(configure=[])。
    ignored 是「看过但决定不暴露」的上游选项：选项 -> 理由。
    两项都由 phprt audit 对照上游选项清单检查，避免漏掉或静默忽略。

    prepare 是在 standalone 构建之前于源码目录执行的 shell 命令，用来给上游源码
    打兼容补丁；写进 signature，命令变化会触发重编。

    env 是这个组件的环境变量；fold 时合并进产品节点。
    """
    return {
        "kind": "component",
        "name": name,
        "product": product,
        "source": source,
        "version": version,
        "url": url,
        "sha256": sha256,
        "modes": modes,
        "needs": needs,
        "tools": tools,
        "dep_configure": dep_configure,
        "prepare": prepare,
        "features": features,
        "ignored": ignored,
        "configure": configure,
        "source_path": source_path,
        "modules": modules,
        "ini": ini,
        "ini_lines": ini_lines,
        "env": env,
        "artifacts": artifacts,
        "desc": desc,
    }


def product(
        name,
        series,
        version = "",
        url = "",
        sha256 = "",
        needs = [],
        deps = {},
        bundled = [],
        bundled_disable = {},
        configure = [],
        make = ["make", "make install"],
        artifacts = ["bin/php"],
        profile = {},
        desc = ""):
    """一个产品（PHP、nginx、CPython）。

    series 是 8.5 这样的系列，补丁和下载地址由 catalog 注入。
    bundled / bundled_disable 是产品版本事实：源码包里自带哪些组件、
    没选中某个组件时用哪个 configure 参数关闭它。
    profile 描述组件怎么挂到产品上、装完怎么整理和验证。
    默认启用集合不在这里，写在 bundles/<产品>/<系列>.toml 的 default_components。
    """
    return {
        "kind": "product",
        "name": name,
        "series": series,
        "version": version,
        "url": url,
        "sha256": sha256,
        "needs": needs,
        "deps": deps,
        "bundled": bundled,
        "bundled_disable": bundled_disable,
        "configure": configure,
        "make": make,
        "artifacts": artifacts,
        "profile": profile,
        "desc": desc,
    }


def php_profile():
    """PHP 产品的 profile：static 折叠进 PHP，shared 用 phpize 单独构建。"""
    return product_profile(
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
        verify = ["{tree}/bin/php", "-v"],
        verify_modules = "{tree}/bin/php -m",
        entry = "myphp-{series}",
        entry_template = "myphp",
        env = {
            "PHP_AUTOCONF": "{tool:autoconf}/bin/autoconf",
            "PHP_AUTOHEADER": "{tool:autoconf}/bin/autoheader",
        },
        audit = "php-config-m4",
        config = "php",
    )
