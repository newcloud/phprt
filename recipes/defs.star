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
        host_tools = [],
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
        "host_tools": host_tools,
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


def tool(name, min = "", probe = ["--version"]):
    """一个宿主机命令要求。

    字符串缩写只检查存在；带 min 时探测版本并要求 >= min。
    probe 是探测命令的参数列表，不经过 shell。
    """
    return {
        "name": name,
        "min": min,
        "probe": probe,
    }


def ensure_dir(path):
    """post_install：在安装树里创建目录。"""
    return {"kind": "ensure-dir", "path": path}


def write_file(path, content, if_missing = False):
    """post_install：写文件；if_missing 时保留已有文件。"""
    return {
        "kind": "write-file",
        "path": path,
        "content": content,
        "if_missing": if_missing,
    }


def copy_ca_bundle(to, candidates):
    """post_install：复制系统 CA 包；候选都不存在时只警告。"""
    return {
        "kind": "copy-ca-bundle",
        "to": to,
        "candidates": candidates,
    }


def write_component_ini(directory, register):
    """post_install：为选中且匹配 register 的组件写 ini，删掉旧 ini。"""
    return {
        "kind": "write-component-ini",
        "dir": directory,
        "register": register,
    }


def _tool_name(item):
    return item if type(item) == "string" else item["name"]


def _copy_dict(value):
    return {key: value[key] for key in value}


def extend_profile(
        base,
        modes = None,
        tools = None,
        defaults = None,
        bundled_path = None,
        runtime = None,
        register = None,
        post_install = None,
        verify = None,
        verify_modules = None,
        entry = None,
        entry_template = None,
        env = None,
        audit = None):
    """在共享 profile 上覆盖少数字段。

    tools 按名字合并（后面的覆盖前面的）；env 按 key 合并；
    其他字段传了才替换。
    """
    out = _copy_dict(base)
    if modes != None:
        out["modes"] = modes
    if defaults != None:
        out["defaults"] = defaults
    if bundled_path != None:
        out["bundled_path"] = bundled_path
    if runtime != None:
        out["runtime"] = runtime
    if register != None:
        out["register"] = register
    if post_install != None:
        out["post_install"] = post_install
    if verify != None:
        out["verify"] = verify
    if verify_modules != None:
        out["verify_modules"] = verify_modules
    if entry != None:
        out["entry"] = entry
    if entry_template != None:
        out["entry_template"] = entry_template
    if audit != None:
        out["audit"] = audit

    if tools != None:
        merged = {}
        order = []
        for item in base.get("tools", []) + tools:
            name = _tool_name(item)
            if name not in merged:
                order.append(name)
            merged[name] = item
        out["tools"] = [merged[name] for name in order]

    if env != None:
        merged_env = _copy_dict(base.get("env", {}))
        for key in env:
            merged_env[key] = env[key]
        out["env"] = merged_env
    return out


def backup(relative_dir, name):
    """组件登记文件的备份规则。

    relative_dir：相对工具目录（默认 .phprt，构建后的安装树所在），不是项目根。
    name：文件名模板，支持 {timestamp}/{component}/{mode}。
    """
    return {"relative_dir": relative_dir, "name": name}


def product_profile(
        modes = {},
        tools = [],
        defaults = {},
        bundled_path = "",
        runtime = {},
        register = {},
        backup = {},
        post_install = [],
        verify = [],
        verify_modules = "",
        entry = "",
        entry_template = "",
        env = {},
        audit = "none"):
    """产品的组件挂载协议、运行时整理、验证和入口。

    tools：产品总是需要的宿主机命令，可带最低版本。
    modes：挂载方式 -> mode(...)。
    defaults：组件来源（bundled / external）-> 默认挂载方式。
    register：挂载方式 -> 登记动作；目前只有 php-ini。
    backup：组件登记文件的备份规则 backup(relative_dir, name)；不写就不备份。
    post_install：安装后动作，见 ensure_dir / write_file / copy_ca_bundle /
                  write_component_ini。
    verify / verify_modules：产品启动验证；verify_modules 用来检查组件模块。
    entry / entry_template：项目入口名模板和产品目录下的模板文件名。
    env：产品专用环境变量，支持 {tree} 和 {tool:<dep>}。
    audit：选项来源，php-config-m4 / configure-help / none。
    """
    return {
        "modes": modes,
        "tools": tools,
        "defaults": defaults,
        "bundled_path": bundled_path,
        "runtime": runtime,
        "register": register,
        "backup": backup if backup else None,
        "post_install": post_install,
        "verify": verify,
        "verify_modules": verify_modules,
        "entry": entry,
        "entry_template": entry_template,
        "env": env,
        "audit": audit,
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
        host_tools = [],
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
        "host_tools": host_tools,
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
        targets = {},
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
        "targets": targets,
        "desc": desc,
    }

