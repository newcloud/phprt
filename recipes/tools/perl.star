def target(ctx):
    # perl 只由系统提供：catalog 里没有 [perl]，所以这里不会下载、也不会编译。
    # 这里只声明"怎么检查 perl 是否真的能用"，使用方按名字点单
    # （见 dep/openssl.star 的 tool("perl", ..., feature_checks = ["findbin"])）。
    return tool_target(
        name = "perl",
        feature_checks = {
            # findbin：openssl 的 Configure 开头就 use FindBin。
            # -MFindBin 让 perl 加载这个模块，-e1 跑一个空程序；模块缺失时
            # 退出码非 0，phprt 就判这条命令不可用。
            # Debian/Ubuntu 的 perl-base、Fedora 的 perl-interpreter 都只有命令
            # 没有这个模块；装发行版的完整 perl 包能补上，所以失败用
            # need_packages：它会并进那条统一的 dnf/apt install 提示。
            # reason 是给人看的一句话，打在提示里、也作为 plan 里“无法解析”的原因；
            # 不写就只报检查命令和 Can't locate FindBin.pm 那一长串。
            # 如果哪天失败是装包解决不了的，改成 error_msg("...") 直接打印。
            "findbin": feature_check(
                run = ["-MFindBin", "-e1"],
                on_fail = need_packages(
                    ["perl"],
                    reason = "当前 perl 模块不全，需要安装完整的 perl 包",
                ),
            ),
            # open：libxcrypt 的 build-aux/scripts/expand-selected-hashes 里 use open。
            # 只随 libxcrypt 构建出现；Debian/Ubuntu 的 perl-base 同样缺这个模块。
            "open": feature_check(
                run = ["-Mopen=:std", "-e1"],
                on_fail = need_packages(
                    ["perl"],
                    reason = "当前 perl 模块不全，需要安装完整的 perl 包",
                ),
            ),
        },
    )


TARGET = target
