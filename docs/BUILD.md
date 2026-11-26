# 编译与安装

`install` / `update` 在解压之后按构建顺序编译源码，产物安装到
`.phprt/<产品>-<产品版本>/`（工具目录默认 `<项目根>/.phprt`，如 `/www/blog/.phprt`；
名字可由 `phprt.toml` 的 `local_internal_dir` 或 `--internal-dir` 改，可绝对或相对），
最后按产品 profile 和选中的构建目标做运行时整理和验证；全局模式（`--global --prefix <目录>`）直接装到 prefix，不生成项目入口。

PHP 选中 `cli` target 时还会生成项目入口 `myphp-<系列>`；只选 `fpm` / `cgi` 时不生成。
`--downloads` 在下载阶段结束，不进入本阶段。

## 1. 顺序

```text
tool（宿主不满足时） → dep → fold 组件（只贡献产品 configure 参数） → product
  → standalone 组件 → runtime（收库、rpath）
  → post_install（目录、文件、CA、ini）
  → verify
  → entry
  → 完成标记 `.phprt/<产品>-<版本>/.phprt-installed`
```

- 拓扑序由求解器决定，构建阶段不再排序。
- fold 组件不单独执行命令。选中的 fold 组件把 recipe 的 `configure` 和 profile
  的 fold 模板交给产品的 `./configure`；没选中、或选中了 standalone 的 bundled
  组件使用产品 recipe `bundled_disable` 里的关闭参数。同一个组件只出现一组参数。
- standalone 组件在产品之后构建：PHP 需要 `php-config`、`phpize` 和产品源码里的组件目录；
  nginx dynamic 模块是 fold，不单独构建。
- 工具在下载前按节点解析：宿主满足就用宿主，否则才排一个 `tool:` 节点。需要构建的
  工具节点统一排在最前面（工具只依赖工具，前置就是拓扑序）；它们只进 `PATH`，
  不参与链接和运行时收库。宿主工具不产生节点。

完成标记是整条流水线（含产品校验和写入口）成功后的最后一步，用临时文件加改名
原子写入，内容是补丁号、方案指纹、解析出的版本指纹与当时的方案命令（组件、mode、
feature、依赖转授、版本线、绑定），以及每个组件在这棵树里留下的产物路径和 ini 指纹；
`resolution=` 只用于比较，定义见 [LOCK.md](LOCK.md) 第 9 节，字段格式见
[STATE.md](STATE.md#7-安装树与完成标记)。
中途失败不会留下标记，所以“目录在、标记不在”说明上次没装完。

## 2. 每个节点

| 类型 | 工作目录 | 命令 |
| --- | --- | --- |
| tool | `build/<名字>/<补丁>` | recipe 的 configure + make，装到 `build/tools/<名字>-<补丁>` |
| dep | `build/<名字>/<补丁>` | recipe 的 configure + make |
| product | `build/<产品>/<补丁>` | `./configure <全部参数>` + make |
| bundled standalone 组件 | `build/<名字>/<产品>-<产品版本>` | 从产品源码的 `source_path` 拷贝 → 组件 `prepare` → profile 的 steps |
| external standalone 组件 | `build/<名字>/<补丁>` | profile 的 steps |
| fold 组件 | 不执行 | 只贡献源码路径和 configure 参数 |

- PHP standalone 组件的 steps 是 `phpize` → `configure --with-php-config=... {flags}` →
  `make` → `make install`。
- 编进产品的 fold 组件不写登记文件；PHP 的 shared 组件写
  `{tree}/etc/conf.d/<ini>`。
- 从 shared 切到 static 时，构建阶段会删掉该组件的旧 ini，避免重复加载。

## 3. 环境

每个节点执行命令时注入以下变量：

| 变量 | 内容 |
| --- | --- |
| `CPPFLAGS` | 当前节点依赖闭包的 `-I<dep>/include` |
| `LDFLAGS` | 当前节点依赖闭包的 `-L<dep>/lib` |
| `PKG_CONFIG_PATH` | 依赖的 `lib/pkgconfig` 和 `share/pkgconfig` |
| `LD_LIBRARY_PATH` | 产品 profile 的 `runtime.lib_dir`（非空时）和依赖的 `lib` / `lib64` |
| `PATH` | `{tree}/bin`、本节点解析到的 built 工具 `bin`，再接原有 PATH |
| `CFLAGS` / `CXXFLAGS` | `-O2`，再接 recipe / env，最后追加选中的 toolchain 档位参数 |
| `MAKEFLAGS` | `-j<parallel>`，默认 CPU 数 - 1（最少 1），见 `phprt.toml` |
| `CMAKE_BUILD_PARALLEL_LEVEL` | 同上的并发数，给 `cmake --build` 用 |

表格里“再接原有 PATH”只发生在执行时；宿主的 `CFLAGS` / `CXXFLAGS` 也在执行时
**前置**到工具生成的值前面，保证 toolchain 档位参数最后生效。宿主 `PATH` /
`CPPFLAGS` / `LDFLAGS` / `CFLAGS` / `CXXFLAGS` 不参与签名，所以换 shell、
上 CI、`.bashrc` 里多一条路径都不会让整棵树重编；反过来，改了这些值也不会自动触发
重编，要重编用 `install --force` 或删 `build/`。

`MAKEFLAGS` / `CMAKE_BUILD_PARALLEL_LEVEL` 同样只用于执行，不进签名：并行度是机器
属性，改 `parallel`、加 `--parallel`、换一台核数不同的机器都不该让整棵树重编。
`make -j1` 这类 recipe 里写死的参数仍然优先于 `MAKEFLAGS`（LuaJIT 就这么写）。

并行度只有一个来源：`--parallel N`，或者 `phprt.toml` 的 `parallel`（CLI 优先，都没写用
CPU 数 - 1）。phprt 把它写成 `MAKEFLAGS` 和 `CMAKE_BUILD_PARALLEL_LEVEL` 传给子进程，
所以 **shell 里 `export MAKEFLAGS=4`、`export CMAKE_BUILD_PARALLEL_LEVEL=4` 都不生效**。

产品 profile 的 `env` 再追加产品专用变量（PHP 的 `CC` / `CXX`）。`{tool:NAME}` 只在
当前节点的 `tools` 含该工具、且工具解析到 built 版本时展开成
`build/tools/<名字>-<版本>`；宿主提供时跳过这个 env 项，让宿主 PATH 自己找到命令。
fold 组件的 `env` 合并进产品节点（如 lua-nginx-module 的 `LUAJIT_INC` / `LUAJIT_LIB`）。

选中的 toolchain 档位把 `cflags` / `cxxflags` 追加进产品、依赖、组件节点的
`CFLAGS` / `CXXFLAGS`；**不作用于 tool 节点**（工具跨产品共享）。工具节点只带
`{tool}` 依赖的 `bin`，不带产品树、`PKG_CONFIG_PATH` 和 `LD_LIBRARY_PATH`。

构建阶段使用绝对路径；`plan` 的输出仍然用相对项目根的路径，便于阅读。
`--root` 默认是 `.`，解析出的工具前缀（`./build/tools/<名字>-<版本>`）和 `{tool:NAME}`
在注入子进程环境前会换成绝对路径：子进程的工作目录是解压目录，相对前缀在那里找不到。

state 里有 `build`（OCI 实例）时，上面这些命令在实例里执行：变量、顺序、签名和
路径规则都不变，源码和安装树用宿主绝对路径挂进实例。实例怎么建、工具怎么装见
[BUILD-IMAGES.md](BUILD-IMAGES.md) §6。

## 4. 跳过与重编

每个工作目录写 `.phprt-sig`，内容包含：

- 节点 id、版本、源码 `url` / `sha256`、挂载策略；
- 本节点实际使用的工具：宿主写 `tool=NAME provider=host`，built 写
  `tool=NAME provider=store:<版本> sig=<tool-sig>`；
- 构建环境：OCI 实例写 `build=oci:<digest>`，宿主不写这一行；实例名不进签名，
  见 [BUILD-IMAGES.md](BUILD-IMAGES.md) §7；
- configure 程序和全部参数；
- 组件 `prepare` / profile mode 的 steps / make 命令（按声明顺序）；
- 注入的环境变量（只算工具生成的部分，见第 3 节；`MAKEFLAGS` / `CMAKE_BUILD_PARALLEL_LEVEL` 不算）；
- 预期的产物路径；
- `copy_from` 源码目录的内容指纹（相对路径 + 内容 hash，软链记目标）；
- 依赖闭包里每个 dep 的名字、版本和它的构建签名。

工具的签名 `tool-sig` 是单独一份：节点 id、版本、url / sha256、实际编译器的
`cc + 版本 + triple`、configure / make 程序与参数、编译相关环境（`CFLAGS` /
`CXXFLAGS` / `CPPFLAGS` / `LDFLAGS` 白名单），以及依赖工具的签名。`PATH` /
`PKG_CONFIG_PATH` / `LD_LIBRARY_PATH` / `MAKEFLAGS` /
`CMAKE_BUILD_PARALLEL_LEVEL` 和宿主执行时追加的值不进 `tool-sig`。工具签名变化只重编
实际使用它的节点；工具不是版本解析的一部分，不进 `resolution=` 和方案指纹。

依赖签名只按实际依赖闭包（`needs` / 已启用 feature 的 `needs`）计算，
不把整个 Registry 的 recipe 塞进签名；未选中的 feature 不进签名。
签名不含时间戳、临时目录和随机数，同样的输入必然得到同样的签名。

签名一致且产物齐全时跳过。签名变化时先执行 `make distclean`（忽略失败），
再重新 configure / make。源码目录在 `build/`，删除它即可强制重新开始。

环境变量按节点裁剪，只放和这个节点产物有关的项。例如只有 `tools` 里有 autoconf 的
standalone 组件节点会拿到 autoconf 的 `bin`，dep 节点不会因为加了一个 phpize 类组件
而重编。

## 5. 日志与失败

- 每个节点一份日志：`build/logs/<kind>-<name>-<version>.log`；名字和版本相同时只写一次。
- 命令失败时打印日志末尾 40 行，退出 1，不写 state。
- 已经完成的节点、已解压的源码和已安装的树都会保留，重跑时按签名跳过。

## 6. 运行时整理

所有构建完成后，按产品 profile 处理：

1. `runtime.lib_dir` 非空时，把运行时依赖（`needs` / 已启用 feature 的 `needs` 闭包，
   不含 `tools`）的 `lib/*.so*`（含软链）复制到 `{tree}/<runtime.lib_dir>`。
   只编静态库的依赖（如 nginx 的 `libxcrypt`）没有 `.so` 可复制，靠产品自己的
   `-l` 参数静态链进可执行文件，不进入 `runtime.lib_dir`。系统库（glibc /
   libcrypt）不复制，由运行环境提供；跨发行版运行库差异见
   [BUILD-IMAGES.md](BUILD-IMAGES.md) §10.1。
2. 用 `patchelf --force-rpath` 按 `runtime.rpath` 设置安装树内的 rpath。patchelf 也是
   构建期工具：本产品解析到 built 版本时用 `build/tools/patchelf-<版本>/bin/patchelf`，
   宿主满足时用宿主 PATH 里的 `patchelf`：
   - PHP：`bin/*` → `$ORIGIN/../lib/phprt`；
     `lib/php/extensions/*/*.so` → `$ORIGIN/../../../phprt`。
   - nginx：`sbin/nginx` / `modules/*.so` → `$ORIGIN/../lib/phprt`。
3. 按 profile 的 `post_install` 执行动作：`ensure_dir` / `write_file` /
   `copy_ca_bundle` / `write_component_ini`。PHP 的 `php.ini`、`00-ssl.ini`、CA 和
   shared 组件 ini 都在这里生成；动作内容见 [PRODUCT.md](PRODUCT.md)。产品声明了
   targets 时，再按声明顺序执行选中 target 的 `post_install`。
4. 运行校验：产品没有 targets 时用 profile 的 `verify`；有 targets 时逐个运行选中
   target 的 `verify` / `verify_modules`。
5. 写入入口：产品没有 targets 时按 profile 的 `entry` / `entry_template`；有 targets
   时取选中 target 里声明了 `entry` 的那一个，多个同时声明则报错。PHP 的 `cli`
   target 写 `myphp-<系列>`，项目里只有一个 PHP 系列时再写 `myphp`；只选 `fpm` 时
   不生成项目入口。nginx 不生成入口。

`myphp-<系列>` 会设置 `PHPRC`、`PHP_INI_SCAN_DIR`、`SSL_CERT_FILE`、`PATH` 和
`extension_dir`，所以整棵树搬走后仍能用；入口里写死安装树目录，不需要
`.phprt/current`。

## 7. 构建工具

工具清单来自 recipe，不写死在 Rust：

| 来源 | 内容 |
| --- | --- |
| 产品 profile 的 `tools` | 产品 / 依赖 / 组件节点的基线，可带版本区间，如 `tool("make", min = "4.0")` |
| dep / component / tool 的 `tools` | 该节点自己的构建期工具，如 cmake、nasm、autoconf |
| feature 的 `requires.commands` | 选中 feature 时检查的宿主机命令（只做选择阶段门禁） |
| `recipes/tools/*.star` | 可从源码构建的工具；catalog 里每条线都是可选版本 |

解析按节点进行：同名要求取交集（`min` 取大、`max` 取小），宿主优先，其次复用
`build/tools`，最后从 catalog 取满足区间的最高版本构建。同名工具允许多版本并存，
按 `(name, version)` 去重。工具节点不继承产品 profile 的 `tools`，否则无法跨产品共享。

gcc / g++ 不在这里：它们是 toolchain，只探测、不下载；`tools` 里写编译器名会在
加载时报错，见 [TOOLS.md](TOOLS.md) §4。

libaom 的 configure 也接受 yasm，但 phprt 只检查 nasm（yasm 已停止维护）。

解析在下载源码之前执行；`--downloads` 只把本次解析到、需要构建的工具源码加进下载，
不检查、不构建。版本探测和比较见 [PRODUCT.md](PRODUCT.md)。

宿主缺命令时，`install` / `add` / `remove` 在下载源码之前一次列出本次方案缺的全部命令，
并按发行版给出安装命令：

```text
error: 宿主缺少构建命令：gcc g++ cmake make m4
这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。
Fedora Linux 43（dnf）上执行：
  sudo dnf install gcc gcc-c++ cmake make m4
```

只提示，不执行；包名映射和发行版识别规则见 [HOST-COMMANDS.md](HOST-COMMANDS.md)。
用构建镜像时，这段检查在实例里做，缺的工具按同一份映射直接装进实例，见
[BUILD-IMAGES.md](BUILD-IMAGES.md) §6.1。

gd 的图片依赖靠 pkg-config 定位：PHP 8.5 的 `ext/gd/config.m4` 用
`PKG_CHECK_MODULES` 查找 `libpng` / `libjpeg` / `libwebp` / `libavif` / `freetype2`，
`zlib` 由 `PHP_SETUP_ZLIB` 要求 `>= 1.2.11`。v3 的 `PKG_CONFIG_PATH` 会按当前节点
依赖闭包自动加上各 dep 的 `lib/pkgconfig` 和 `share/pkgconfig`。

## 8. 验收

```bash
rm -rf .phprt build

# PHP 8.5 默认集
cargo run -- install php@8.5
./myphp-8.5 -v
./myphp-8.5 -m | grep -E '^(zlib|sqlite3|pdo_sqlite|mysqli|pdo_mysql|mbstring|zip|sodium|gettext|pcntl|posix|sockets|shmop|sysvmsg|sysvsem|sysvshm)$'

# 官方测试用例（验收标准）：在源码目录跑 PHP 自带的 run-tests 全量套件
make test TESTS="-j$(nproc)"

# 只装 PHP 本体：禁用有依赖库的三个默认组件
cargo run -- install php@8.5 --disable-default-component openssl --disable-default-component curl --disable-default-component libxml

# 再显式加 swoole / xdebug
cargo run -- add php@8.5 swoole
cargo run -- add php@8.5 xdebug
cargo run -- install php@8.5
./myphp-8.5 -m | grep -E 'openssl|curl|swoole|xdebug'

# curl 动态编译，并启用 SSL / nghttp2
cargo run -- install php@8.5 --component 'curl=mode:shared,dep:libcurl[ssl,nghttp2]'

# gd：默认读取 [component_presets].gd，自动带 JPEG/PNG/WebP/FreeType
cargo run -- install php@8.5 --component gd
cargo run -- install php@8.5 --component 'gd=feature:[]'          # 只保留 PNG
cargo run -- install php@8.5 --component 'gd=dep:jpeg[simd]'      # preset + JPEG SIMD
cargo run -- install php@8.5 --component 'gd=feature:[jpeg,webp,freetype,avif]'
./myphp-8.5 -r 'var_export(gd_info());'
grep -E 'SIMD extensions' build/logs/dep-jpeg-3.1.4.1.log

# nginx：默认依赖 PCRE2 + OpenSSL，再动态加 lua 模块
cargo run -- install nginx@1.29
.phprt/nginx-1.29.8/sbin/nginx -V
cargo run -- add nginx@1.29 lua-nginx-module
cargo run -- install nginx@1.29
ls .phprt/nginx-1.29.8/modules/

# 只下载，不编译
cargo run -- install php@8.5 --downloads --offline
```

`make test` 在离线环境有两个注意点：

- run-tests 跑完会向 qa.php.net 发报告，`fwrite()` 报 Bad file descriptor、`make`
  返回非零；这是环境问题，验收只看 Tests passed / failed / skipped 计数。
- 8.1 的 `run-tests.php` 有上游 bug：没编 php-cgi / phpdbg 时 `$php_cgi` / `$phpdbg`
  仍被引用（`Undefined variable $php_cgi`），测试跑到 CGI 段就中断。8.2 起改成
  `?? null`，8.1 分支 EOL 没有回填。recipe 不改上游源码，手动把
  `build/php/php-8.1.34/run-tests.php` 第 1900 ~ 1906 行换成：

```php
    $php_cgi = $env['TEST_PHP_CGI_EXECUTABLE'] ?? null;
    $phpdbg = $env['TEST_PHPDBG_EXECUTABLE'] ?? null;
```

依赖库应该从树内加载：

```bash
ldd .phprt/php-8.5.12/bin/php | grep libssl
# .../.phprt/php-8.5.12/lib/phprt/libssl.so.3
```

## 9. 不在范围

LTO、`--static-all`、lock 文件、多产品共享安装树、增量链接优化。
其他 PHP 系列的 recipe 使用同一套流程，目前只在 8.0 ~ 8.6 上验收。
