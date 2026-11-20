def ssl_dir(ctx):
    # 默认 CA 目录跟发行版走；认不出发行版时退回安装前缀，不写死宿主路径。
    host = ctx.get("host") or {}
    family = host.get("family", "")
    if family == "rpm":
        return "/etc/pki/tls"
    if family == "deb":
        return "/etc/ssl"
    return "{prefix}/ssl"


def target(ctx):
    self = ctx["self"]
    line = self["line"]
    openssldir = "--openssldir=" + ssl_dir(ctx)

    if line == "1.1.1":
        configure = [
            "./config",
            "--prefix={prefix}",
            openssldir,
            "--libdir=lib",
            "shared",
            "no-tests",
        ]
        artifacts = ["lib/libssl.so.1.1", "lib/libcrypto.so.1.1"]
    else:
        major = line.split(".")[0]
        configure = [
            "./Configure",
            "--prefix={prefix}",
            openssldir,
            "--libdir=lib",
            "shared",
            "no-tests",
        ]
        artifacts = ["lib/libssl.so." + major, "lib/libcrypto.so." + major]

    return dep(
        name = "openssl",
        version = self["version"],
        url = self["url"],
        sha256 = self["sha256"],
        needs = [],
        # openssl 的 Configure 是 Perl 脚本，开头就用 FindBin。Debian/Ubuntu 的
        # perl-base、Fedora 的 perl-interpreter 都只带命令不带这个模块，所以这里
        # 声明一个具名检查 findbin：检查怎么写、失败后装什么包，都定义在
        # recipes/tools/perl.star 里。check 不过时按"缺命令"提示装完整 perl。
        tools = [tool("perl", min = "5.20", feature_checks = ["findbin"])],
        configure = configure,
        make = ["make", "make install_sw"],
        artifacts = artifacts,
        desc = "OpenSSL 库，供 PHP openssl 扩展链接",
    )


TARGET = target
