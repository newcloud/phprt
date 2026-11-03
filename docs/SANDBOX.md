# 沙箱（bwrap）

`./myphp` 可以在 [bubblewrap](https://github.com/containers/bubblewrap) 里跑：

```bash
./myphp --sandbox worker.php      # 这一次进沙箱
phprt init --sandbox --rw var     # 固化到 ./myphp（之后连 .rr.yaml 都不用改）
./myphp --sandbox-check           # 自检
./myphp --no-sandbox ...          # 这次不进（固化的是 on 时）
```

需要系统里有 `bwrap`：

```bash
sudo dnf install bubblewrap    # Fedora
sudo apt install bubblewrap    # Debian/Ubuntu
```

## 它做了什么

| 项 | 沙箱内 |
| --- | --- |
| 根目录 | **空的**（`--tmpfs /`）：`/home`、`/root`、别的项目统统不存在 |
| 系统目录 | `/usr` 只读；`/bin /sbin /lib /lib64` 按宿主的软链或真目录原样处理 |
| 系统库目录 | `/usr/lib64` 等**清空**，只放回 `sandbox.libs` 白名单里的文件 |
| `/etc` | 只给 `hosts resolv.conf nsswitch.conf passwd group localtime pki ssl` |
| 项目目录 | **只读**（`myphp`、`vendor/`、`.git/` 都改不了） |
| 可写目录 | 只有 `PHPRT_RW_DIRS` 里列的那些（如 `var`） |
| 安装根 | 只读（`.phprt/` 里的 PHP 本体不可改） |
| `/tmp`、`/home` | 独立 tmpfs，进程结束就没了 |
| 进程 | 独立 PID 命名空间（PHP 看到自己是 pid 2），看不到宿主进程 |
| 网络 | 默认共享（服务要 bind 8080）；`--no-net` 用 `--unshare-net` |

实测（PHP 8.6，33 个模块都正常）：

```
$ ./myphp --sandbox -r 'echo file_exists("/usr/lib64/libcurl.so.4");'
                      # → false（系统的 libcurl 8.15 被挡掉了）

$ ./myphp --sandbox -r 'file_put_contents("hack.txt","x");'
                      # → 被拒绝（项目只读）

$ ./myphp --sandbox -r 'file_put_contents("var/log/x","x");'
                      # → ok
```

## 可写目录（`PHPRT_RW_DIRS`）

沙箱里**除这些目录外全部只读**。相对项目根，空格分隔：

```bash
phprt init --sandbox --rw var --rw var/log      # 两个目录
phprt init --sandbox --rw var,vendor            # 逗号也行
PHPRT_RW_DIRS="var storage" ./myphp worker.php  # 临时多开一个
```

- 目录不存在时 `./myphp` 会先 `mkdir -p`（bwrap 的 bind 源必须存在）
- `var` 已经包含 `var/log`，两个都列只是写清楚；想「日志可写、数据库只读」就只列 `var/log`
- 安装根永远只读，即使它在可写目录里面（挂载顺序保证）
- 要在沙箱里跑 `composer install` 就得把 `vendor` 也列上（缓存固定在 `/tmp/composer`，用完即弃）

## 系统库白名单（`sandbox.libs`）

`phprt build` 每次都会重新生成 `<安装根>/php-<版本>/sandbox.libs`：

```
# = ldd 扫 php + 全部扩展后，落在安装根之外的路径
/lib64/ld-linux-x86-64.so.2
/lib64/libc.so.6
/lib64/libm.so.6
/lib64/libgcc_s.so.1
/usr/lib64/gconv               # ← dlopen 才会用到，ldd 看不到
/usr/lib64/libnss_files.so.2   #   DNS / 用户查询
/usr/lib64/libnss_dns.so.2
/usr/lib/locale/locale-archive
```

含义：**除了这几个 glibc 文件，任何系统库在沙箱里都不存在**。所以
`libcurl`、`libssl`、`libicu`、`libxml2` 这些只能来自 `deps/`；
如果某个扩展偷偷链了系统库，`./myphp --sandbox -v` 会当场报
`error while loading shared libraries: ...`，而不是悄悄跑起来。

要放行额外的库（不推荐，正确做法是把它编进 `deps/`）：

```bash
PHPRT_SANDBOX_LIBS="/usr/lib64/libnvidia-ml.so.1" ./myphp --sandbox -r '...'
```

## 参数一览

| 变量 / 参数 | 默认 | 说明 |
| --- | --- | --- |
| `PHPRT_SANDBOX` / `--sandbox` / `--no-sandbox` | `off` | 是否进沙箱 |
| `PHPRT_RW_DIRS` / `--rw` | `var` | 沙箱里可写的目录（相对项目根） |
| `PHPRT_SANDBOX_LIBS` | 空 | 额外放行的系统文件/目录 |
| `PHPRT_SANDBOX_OPTS` | 空 | 追加给 bwrap 的参数（逃生口） |
| `--no-net` | 共享网络 | 改用 `--unshare-net` |

`PHPRT_SANDBOX_OPTS` 常用的两个：

```bash
# 退回「只隔离文件系统、不锁系统库」（排查用）
PHPRT_SANDBOX_OPTS="--ro-bind /usr/lib64 /usr/lib64" ./myphp ...

# 把 /tmp 换成磁盘目录（大文件上传别占内存）
PHPRT_SANDBOX_OPTS="--bind /var/tmp/phprt /tmp" ./myphp ...
```

## 排错

| 现象 | 原因 / 处理 |
| --- | --- |
| `沙箱需要 bubblewrap（bwrap）` | 装 bubblewrap |
| `bwrap 不能创建用户命名空间` | Ubuntu 24.04+：`sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0` |
| `缺少沙箱白名单 .../sandbox.libs` | 跑 `phprt build --php <版本>` 重新生成 |
| `error while loading shared libraries: libxxx` | 该库不在 `deps/`：要么把它编进 `deps/`，要么临时 `PHPRT_SANDBOX_LIBS` 放行 |
| 写入被拒绝 | 该目录不在 `PHPRT_RW_DIRS` 里 |
| DNS 解析失败 | 检查 `/etc/resolv.conf` 是否被换成了别的位置（systemd-resolved 的 stub 是 `127.0.0.53`，沙箱里共享网络所以仍可用） |

## 已知取舍

- **RoadRunner 的 `max_worker_memory`**：RR 在沙箱外，量到的是 bwrap 进程，不是 PHP 进程，
  内存上限判断偏松。想精确就把 `bin/rr` 也放进沙箱（`bwrap ... ./bin/rr serve`）。
- **`/tmp` 是 tmpfs**（占内存），上传大文件用上面的 `--bind` 换盘。
- **Composer 缓存**固定在 `/tmp/composer`，沙箱里跑 `composer install` 每次都会重新下载。
- **glibc 是唯一真正的外部依赖**：编译机的 glibc 版本不能高于运行机。
  想彻底摆脱要用 musl 静态编译（`static-php-cli` 那条路），扩展生态会受限。
- 沙箱只包 PHP 进程。要是把 RR 也包进去，`/proc` 里的内存统计才准，但启动命令会变长。
