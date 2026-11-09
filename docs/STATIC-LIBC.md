# 全静态 glibc：可行性与跨发行版发布

- 状态：分析文档，未实现
- 日期：2026-11-09
- 相关：[STATIC-ALL.md](STATIC-ALL.md)、[STATIC-ALL-LTO.md](STATIC-ALL-LTO.md)
- 范围：讨论把 `libc`、`libm` 等系统库也静态链接的难度，以及产物发布到
  其他 Linux 系统会遇到的问题；本文不定义新选项，也不改构建代码。

## 1. 结论

**能编出来，但“全静态 glibc”不等于“任何 Linux 都能正常跑”。**

- 构建难度中等：需要系统静态库、所有依赖的 `.a`，以及链接、wheel 化、
  `verify` 的适配。
- 运行风险高：NSS/DNS、iconv/gconv、`dlopen` 是 glibc 全静态的硬伤。
- 当前 `--static-all` / `--static-all-lto` 仍是 **glibc 动态**；LTO 只影响
  代码生成，不改变运行时依赖。
- 如果目标是跨发行版发布，优先考虑 **musl 静态** 或 **最小运行包**；
  glibc 全静态只适合目标环境明确、能接受功能限制的场景。

## 2. 当前边界

| 选项 | PHP 本体 | 依赖 | glibc | 产物 |
| --- | --- | --- | --- | --- |
| `--static` | 支持的扩展编入 | 动态 `.so` | 动态 | 依赖 `deps/` 或 `lib/phprt/` |
| `--static-all` | 扩展全部编入 | 静态 `.a` | 动态 | 单个 `bin/php` |
| `--static-all-lto` | 扩展全部编入 + LTO | 静态 `.a` + LTO | 动态 | 单个 `bin/php` |
| 全静态 glibc（未实现） | 扩展全部编入 | 静态 `.a` | 静态 | 真正无动态依赖 |

实测 `v2default`（`--static-all`）：

```text
$ readelf -d bin/php | grep NEEDED
  (NEEDED)  libm.so.6
  (NEEDED)  libc.so.6
```

构建机：Fedora 43，glibc 2.42。`bin/php` 的动态符号最高引用到
`GLIBC_2.38`。因此当前产物的实际兼容边界是“目标 glibc ≥ 2.38”：

| 目标系统 | 当前 static-all |
| --- | --- |
| Fedora 43（glibc 2.42） | 可用 |
| Ubuntu 24.04（glibc 2.39） | 可用 |
| Debian 12（glibc 2.36） | 通常不可用，缺 `GLIBC_2.38` |
| Ubuntu 22.04（glibc 2.35） | 不可用 |
| CentOS 7（glibc 2.17） | 不可用 |
| Alpine（musl） | 不可用，没有 glibc 动态加载器 |
| distroless / scratch | 不可用，缺 libc 和加载器 |

这张表是“按符号版本判断”的预期，实际以目标机运行为准。

## 3. 构建难度

### 3.1 系统静态库

当前机器没有安装 glibc 的静态档案，连一个 `gcc -static` 的 hello world
都链接不了：

```text
/usr/bin/ld: 找不到 -lc
have you installed the static version of the c library ?
```

需要补齐：

| 库 | Fedora 包 | 用途 |
| --- | --- | --- |
| `libc.a`、`libm.a`、`libresolv.a` | `glibc-static` | libc / libm / DNS |
| `libstdc++.a` | `libstdc++-static` | C++ 依赖（aom、avif、imagick 等） |
| `libcrypt.a` | `libxcrypt-static` | `crypt` 相关探测 |
| `libpthread.a`、`libdl.a`、`librt.a` | `glibc-static` | glibc 2.34+ 兼容空壳 |
| `libnsl.a`、`libutil.a`、`libanl.a` | `glibc-static` | 视 configure 探测结果 |

如果 glibc 没有用 `--enable-static-nss` 构建，`libnss_files.a` /
`libnss_dns.a` 可能不存在，或者不能解决 NSS 问题；需要单独验证。

### 3.2 PHP 链接层

- `LDFLAGS` 增加 `-static`（或 `-static-pie`）。
- PHP 的 CLI 链接走 libtool，可能需要 `-all-static` / `-static` 组合，
  不能只改一个变量。
- configure 的所有探测程序也会变成静态链接；缺 `libcrypt.a`、`libresolv.a`
  时，configure 可能在早期失败。
- `phpdbg`、`phar.phar` 也要一起静态。
- `php-config --ldflags` 不能让后续扩展误以为 PHP 是动态链接的；
  全静态树必须拒绝 `add ext --shared`。

### 3.3 wheel 化与自检

- `wheelize_tree()` 现在会对每个 `bin/*` 执行：
  ```bash
  patchelf --set-rpath '$ORIGIN/../lib/phprt' --force-rpath "$elf"
  ```
  全静态二进制没有 `.dynamic`，`patchelf` 很可能报错；需要跳过静态 ELF，
  或单独分支。
- `verify` 的 `ldd` 检查要能识别 `statically linked`。
- 动态库检查和 RPATH 检查在全静态下应变成“没有 INTERP、没有 NEEDED、
  没有 `.dynamic`”，而不是继续按 glibc 白名单过滤。
- `BUILD-INFO` / `BUILD-STATUS` 需要记录“全静态”状态，避免和
  `--static-all` 混淆。

### 3.4 所有依赖都要有静态库

当前不少 dep recipe 明确 `--disable-static`：

```text
icu gmp imagemagick libffi libpng libsodium
libssh2 libxslt libyaml librdkafka ...
```

全静态要逐个改成 `--enable-static`，其中一些不是改参数就能解决：

| 依赖 | 难度 | 说明 |
| --- | --- | --- |
| zlib、sqlite、openssl、curl、oniguruma | 低 | 当前已产出 `.a` |
| libxml2、libxslt、libyaml、libssh2、gmp、libffi、tidy | 低-中 | 通常支持 `--enable-static` |
| libzip | 中 | 还要 zlib/bzip2/lzma/zstd 的静态档案 |
| libsodium | 低 | 当前 recipe 是 `--disable-static` |
| libpq | 中 | 可关 GSSAPI；Kerberos 静态化很难 |
| OpenLDAP | 中-高 | SASL 插件、TLS、配置 |
| libmemcached | 中-高 | SASL 插件 |
| librdkafka | 高 | zstd/lz4/SASL/SSL 依赖多 |
| mongodb | 高 | 自带 libmongoc/libbson，可能牵涉 snappy/zstd/ICU |
| ICU | 中-高 | `libicudata.a` 或运行时 `.dat`，体积大 |
| gd 图片链 | 高 | aom/avif 是 C++，依赖多，体积大 |
| ImageMagick / imagick | 很高 | coder 模块、`policy.xml`、`delegates.xml` |
| swoole / xdebug | 不可用 | 当前契约是 shared-only，`--static-all` 已排除 |

全静态不是“PHP 加一个 `-static`”，而是整个依赖图都要满足静态约束。

### 3.5 体积与构建成本

- 当前 `--static-all` 的 `bin/php` 约 39 MB。
- 全静态默认集加上 ICU、gd、xsl、ldap、pgsql、ffi、tidy，预计 80–150 MB。
- 再加 imagick、mongodb、rdkafka，可能超过 200 MB。
- 所有依赖都要重编；链接时间和内存都会上升。
- 静态二进制无法靠发行版更新单个库，CVE 修复必须整体重编。

## 4. 运行时风险

| 功能 | 风险 | 原因 | 可能的缓解 |
| --- | --- | --- | --- |
| DNS / `getaddrinfo` | 高 | NSS 模块是运行时 `dlopen` 的 `.so` | 静态 NSS（仅 files+dns）、musl、c-ares |
| `posix_getpwnam` / 用户组 | 高 | NSS（files、sss、ldap） | 静态 NSS 或改用文件 |
| `iconv` | 高 | glibc gconv 模块在 `/usr/lib64/gconv` | 静态 GNU libiconv、只用 mbstring |
| `setlocale` / 本地化格式 | 中 | 目标机 locale archive 不同 | 只用 `C.UTF-8`，或随包带 locale |
| `dl()` / `FFI` | 高 | 静态程序没有可靠的 `dlopen` | 不使用；FFI 基本不可用 |
| OpenSSL legacy / engine | 中 | provider/engine 是动态模块 | 只用默认 provider，或编进主程序 |
| ImageMagick | 高 | coder 模块、`policy.xml`、`delegates.xml` | `--with-modules=no` + 随包配置 |
| `intl` / ICU | 中 | ICU 数据文件或静态数据 | 链接 `libicudata.a` |
| `curl` HTTPS | 中 | CA 路径随发行版不同 | 随包 CA + `curl.cainfo` |
| `proc_open` / `mail` / `exec` | 中 | 依赖目标机 `/bin/sh`、sendmail | 运行环境提供 |
| `DateTime` / 时区 | 低 | PHP 自带 timelib 时区数据 | 通常无需处理 |
| SQLite / mbstring / 默认 OpenSSL | 低 | 已静态链接，无运行时数据 | 保持 |

### 4.1 NSS / DNS：最大的硬伤

glibc 静态程序里的 `getaddrinfo`、`getpwnam`、`getgrnam` 仍依赖 NSS。
NSS 模块（`libnss_files.so.2`、`libnss_dns.so.2`、`libnss_sss.so.2` 等）
是运行时 `dlopen` 的共享库，而且必须和静态链接进去的 glibc 版本匹配。

glibc 自己会给出警告：

```text
Using 'getaddrinfo' in statically linked applications requires at runtime
the shared libraries from the glibc version used for linking
```

实际后果：

- 目标机没有对应模块时，域名解析可能只能读 `/etc/hosts`。
- LDAP/SSSD 用户查询失败。
- 依赖域名解析的 `mysqli`、`pgsql`、`redis`、`mongodb`、`curl` 都可能受影响。

除非 glibc 用 `--enable-static-nss` 构建并静态链接 `nss_files`/`nss_dns`，
但那样也只覆盖 files + DNS，LDAP/SSSD 仍然没有。

### 4.2 iconv / gconv

glibc 的 `iconv_open()` 会从 `/usr/lib64/gconv/` 加载转换模块。目标机没有
匹配的 gconv 时，`iconv()` 对 GB18030、GBK 等编码可能直接失败。

- `mbstring` 相对独立（oniguruma + 自己的编码表），通常不受影响。
- 想彻底解决可以静态链接 GNU libiconv，但又多一个依赖。

### 4.3 dlopen / FFI / 插件

全静态程序没有正常的动态加载器，`dlopen` 不可靠或不可用。受影响的功能：

- PHP 的 `dl()`
- `FFI`
- OpenSSL 的 legacy provider / engine
- ImageMagick 的 coder 模块
- SASL 插件、部分 LDAP/Kerberos 配置

可以全部改成“编进主程序”，但功能会缩水，比如 OpenSSL 的 legacy 算法、
ImageMagick 的动态 coder、SASL 的多种机制。

### 4.4 CA、php.ini、OpenSSL 配置

这些不是“静态链接”本身的问题，但单文件发布一定会遇到：

- `--with-config-file-path` 编译进的是构建树的绝对路径；复制到别的机器后
  找不到 `php.ini`。
- `openssl.cafile` / `curl.cainfo` 指向树内 `etc/ssl/cert.pem`；只复制
  `bin/php` 就没有 CA。
- 各发行版 CA 路径不同：
  - Fedora：`/etc/pki/tls/certs/ca-bundle.crt`
  - Debian/Ubuntu：`/etc/ssl/certs/ca-certificates.crt`
  - Alpine：`/etc/ssl/cert.pem`
- OpenSSL 的 `OPENSSLDIR`、`openssl.cnf`、provider 路径也可能和目标机不一致。

所以“单文件”实际至少要配一个 wrapper，设置 `PHPRC`、`SSL_CERT_FILE`、
`OPENSSL_CONF`，或者随包带 CA/ini。

### 4.5 ICU / ImageMagick / 其他运行时数据

- ICU 如果没静态链接数据，需要 `icudt79l.dat`。
- ImageMagick 即使静态编译，也可能需要 `policy.xml`、`delegates.xml`、
  `colors.xml`。
- fontconfig、SASL、Kerberos、LDAP 都有各自的 `/etc/...` 配置。
- `mail()`、`proc_open()`、`exec()` 需要目标机的 `/bin/sh`、sendmail 等，
  静态 PHP 不会自带。

### 4.6 内核、CPU、架构、安全

- 静态二进制仍然是 x86-64 专用；换架构直接不行。
- 目标内核太老可能缺 `statx`、`clone3`、`faccessat2` 等系统调用；
  glibc/musl 通常有回退，但不保证。
- 当前编译是 `-O2`，没有 `-march=native`，CPU 兼容性还好；如果以后加
  `-march=native`，分发就会挑 CPU。
- `-static` 没有 ASLR；`-static-pie` 需要全 PIC 和 glibc 支持，PHP 未必能
  直接过。
- glibc 是 LGPL，静态链接分发通常要提供可重链接的目标文件/源码；
  libstdc++/libgcc 有 GPLv3 runtime exception；OpenSSL、ICU、ImageMagick
  各有许可要求。
- 静态二进制会让 `LD_PRELOAD`、部分安全工具、`patchelf` 失效。

## 5. 发布场景矩阵

| 场景 | 当前 static-all | 全静态 glibc | musl 静态 |
| --- | --- | --- | --- |
| 同发行版、glibc ≥ 构建机 | 可用 | 可用，但有 NSS/iconv 限制 | 可用 |
| 老 glibc（Debian 12、Ubuntu 22.04、CentOS 7） | 通常不可用 | 可能可用，需实测 NSS/iconv | 可用 |
| Alpine | 不可用 | 可能可用，但没有 glibc NSS/gconv | 可用 |
| distroless / scratch | 不可用 | 需要自带 CA/ini，可能还要 shell | 可用，需自带 CA/ini |
| 不同 CPU 架构 | 不可用 | 不可用 | 不可用 |
| 需要 FFI / dl / ImageMagick / SASL | 可用 | 高风险或不可用 | 同样受限 |

## 6. 推荐路线

### 路线 A：保持 `--static-all`

定位为：

> 同一发行版、目标 glibc ≥ 构建 glibc 时，单文件运行。

在文档中明确写出 glibc 符号版本边界，避免用户误以为“单文件 = 任意 Linux”。

### 路线 B：最小运行包（推荐先做）

不要追求一个文件，打一个 tar.gz：

```text
php
php.ini
ssl/cert.pem
（需要时）ICU 数据 / OpenSSL 配置
```

再用一个 `php` wrapper 设置 `PHPRC`、`SSL_CERT_FILE`。这样能解决 CA、ini、
扩展配置的大部分问题，同时保留 glibc 动态链接。

### 路线 C：musl 静态（真正跨发行版）

用 `x86_64-linux-musl-gcc` 重新构建 PHP 和依赖：

- musl 的 DNS 是内置解析器，不依赖 NSS 模块；
- 不需要目标机有 glibc；
- 但 musl 的 iconv 编码集有限，部分 PECL 扩展要重新适配；
- 静态 musl 下 `dlopen`/FFI 仍然不可用。

如果目标是“放到 Alpine、Debian、CentOS、distroless 都能跑”，这比 glibc
全静态更靠谱。

### 路线 D：glibc 全静态只做实验

如果一定要试，建议单独加一个实验开关（例如 `--static-libc`），并明确限制：

- 只支持 CLI 和有限扩展；
- 不支持 FFI、`dl()`、ImageMagick 动态 coder、SASL 插件；
- DNS 只保证 `/etc/hosts`，或额外静态链接 `nss_files`/`nss_dns`；
- iconv 只保证内置编码；
- 必须自带 CA/ini；
- 在 Ubuntu 20.04/22.04/24.04、Debian 11/12、Alpine、CentOS 7/9 上跑
  矩阵测试。

## 7. 若以后实现的验收清单

### 7.1 构建前

- 检查 `libc.a`、`libm.a`、`libresolv.a`、`libcrypt.a`、`libstdc++.a`
  是否存在；缺失时给出明确的包名提示。
- 检查 `glibc` 是否带静态 NSS；没有时在计划输出里标注限制。
- 确认所有选中依赖都提供 `.a`。

### 7.2 构建后

```bash
file bin/php                 # 期望：statically linked
ldd bin/php                  # 期望：not a dynamic executable
readelf -d bin/php           # 期望：没有 NEEDED
readelf -l bin/php | grep INTERP   # 期望：无输出
readelf --dyn-syms bin/php | grep GLIBC_  # 期望：无输出
```

运行检查：

```bash
php -v
php -m

php -r 'var_dump(gethostbyname("example.com"));'
php -r 'var_dump(getpwnam("root"));'
php -r 'var_dump(iconv("UTF-8", "GB18030", "你好"));'
php -r 'var_dump(setlocale(LC_ALL, "zh_CN.UTF-8"));'
php -r 'var_dump(class_exists("FFI"));'
php -r 'var_dump(extension_loaded("openssl"));'
```

### 7.3 目标系统矩阵

至少覆盖：

- Fedora 43（构建机）
- Ubuntu 20.04 / 22.04 / 24.04
- Debian 11 / 12
- Alpine 3.19 / 3.20
- CentOS 7 / Stream 9
- distroless 或最小 chroot（没有 `/etc/nsswitch.conf`、
  `/usr/lib64/gconv`、`/etc/ssl`）

每个环境至少验证：

- `php -v`、`php -m`
- DNS / 用户查询 / iconv / locale
- `curl https://example.com`
- `PDO sqlite`
- `openssl_encrypt`
- `DateTimeZone`
- `proc_open`
- 只带 `bin/php` 和带完整运行包两种情况

## 8. 未决问题

- 是否接受 glibc 全静态带来的 NSS/iconv/dlopen 功能损失？
- 目标用户是否真的需要跨发行版，还是只需要“同一发行版不带 deps 目录”？
- 如果只需要跨发行版，是否优先投入 musl 工具链，而不是 glibc 全静态？
- 最小运行包是否可以作为 `--static-all` 的默认发布形态？
- 全静态下 `php-config` / `phpize` / `add ext` 的语义如何定义？

在这些问题明确之前，建议保持 `--static-all` 的现有边界，不把 glibc
全静态设为默认行为。
