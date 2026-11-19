LINES = {
    "5.1": {"product": ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"]},
}


def target(ctx):
    self = ctx["self"]
    builtin = ctx["is_bundled"]

    return component(
        product = "php",
        name = "apcu",
        modules = ["apcu"],
        modes = ["shared"],
        tools = [tool("autoconf", min = "2.68")],
        ignored = {
            "--enable-apcu-debug": "调试用途：输出 APCu 内部调试信息",
            "--enable-apcu-clear-signal": "少用：用 SIGUSR1 清空缓存，默认关",
            "--enable-apcu-mmap": "少用：mmap 是默认存储方式，不暴露切换",
            "--enable-apcu-rwlocks": "少用：rwlock 是默认行为，不暴露切换",
            "--enable-apcu-spinlocks": "少用：默认关的自旋锁优化",
            "--enable-coverage": "调试用途：代码覆盖率符号",
            "--enable-valgrind-checks": "调试用途：Valgrind 内存检查",
        },
        configure = ["--enable-apcu"],
        version = ctx["product_version"] if builtin else self["version"],
        url = "" if builtin else self["url"],
        sha256 = "" if builtin else self["sha256"],
        ini = "25-apcu.ini",
        # CLI 下默认不开缓存，这里打开，方便命令行进程共享同一份配置。
        ini_lines = [
            "extension=apcu.so",
            "apc.enabled=1",
            "apc.enable_cli=1",
            "apc.shm_size=32M",
        ],
        desc = "APCu 用户缓存扩展（shared 编译）",
    )


TARGET = target
