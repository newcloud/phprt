# 编译与安装

`install` / `update` 在解压之后按构建顺序编译源码，产物安装到
`.phprt/<产品>-<产品版本>/`，最后按产品 profile 做运行时整理和验证；PHP 还会生成
项目入口 `myphp-<系列>`。`--downloads` 在下载阶段结束，不进入本阶段。

## 1. 顺序

```text
dep → fold 组件（只贡献产品 configure 参数） → product → standalone 组件
  → runtime（收库、rpath）
  → post_install（目录、文件、CA、ini）
  → verify
  → entry
```

- 拓扑序由求解器决定，构建阶段不再排序。
- fold 组件不单独执行命令。选中的 fold 组件把 recipe 的 `configure` 和 profile
  的 fold 模板交给产品的 `./configure`；没选中、或选中了 standalone 的 bundled
  组件使用产品 recipe `bundled_disable` 里的关闭参数。同一个组件只出现一组参数。
- standalone 组件在产品之后构建：PHP 需要 `php-config`、`phpize` 和产品源码里的组件目录；
  nginx dynamic 模块是 fold，不单独构建。
- `tools`（如 autoconf）作为 dep 节点参与顺序，只进 `PATH`，不参与链接。

## 2. 每个节点

| 类型 | 工作目录 | 命令 |
| --- | --- | --- |
| dep | `build/<名字>/<补丁>` | recipe 的 configure + make |
| product | `build/<产品>/<补丁>` | `./configure <全部参数>` + make |
| bundled standalone 组件 | `build/<名字>/<产品>-<产品版本>` | 从产品源码的 `source_path` 拷贝 → profile 的 steps |
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
| `LD_LIBRARY_PATH` | `{tree}/lib/phprt` 和依赖的 `lib` / `lib64` |
| `PATH` | `{tree}/bin`、工具 dep 的 `bin`，再接原有 PATH |
| `CFLAGS` / `CXXFLAGS` | `-O2`，再接用户已有的值 |
| `MAKEFLAGS` | `-j<nproc>` |
| `PERL5LIB` | 仓库的 `vendor/perl`（补系统 perl 缺的纯 Perl 模块） |

产品 profile 的 `env` 再追加产品专用变量（PHP 的 `CC` / `CXX` / `PHP_AUTOCONF` /
`PHP_AUTOHEADER`；8.0 的 `CFLAGS`）；引用了不在本次构建里的工具时跳过。fold 组件的
`env` 合并进产品节点（如 lua-nginx-module 的 `LUAJIT_INC` / `LUAJIT_LIB`）。

构建阶段使用绝对路径；`plan` 的输出仍然用相对项目根的路径，便于阅读。

## 4. 跳过与重编

每个工作目录写 `.phprt-sig`，内容包含：

- 节点名、版本、挂载策略；
- configure 程序和全部参数；
- 组件 steps / make 命令；
- 注入的环境变量；
- 预期的产物路径。

签名一致且产物齐全时跳过。签名变化时先执行 `make distclean`（忽略失败），
再重新 configure / make。源码目录在 `build/`，删除它即可强制重新开始。

环境变量按节点裁剪，只放和这个节点产物有关的项。例如 `PHP_AUTOCONF` 只给产品节点和
组件节点，dep 节点不会因为加了一个 phpize 类组件而重编。

## 5. 日志与失败

- 每个节点一份日志：`build/logs/<kind>-<name>-<version>.log`；名字和版本相同时只写一次。
- 命令失败时打印日志末尾 40 行，退出 1，不写 state。
- 已经完成的节点、已解压的源码和已安装的树都会保留，重跑时按签名跳过。

## 6. 运行时整理

所有构建完成后，按产品 profile 处理：

1. `runtime.lib_dir` 非空时，把各 dep 的 `lib/*.so*`（含软链）复制到 `{tree}/lib/phprt`。
2. 用 `patchelf --force-rpath` 按 `runtime.rpath` 设置安装树内的 rpath：
   - PHP：`bin/*` → `$ORIGIN/../lib/phprt`；
     `lib/php/extensions/*/*.so` → `$ORIGIN/../../../phprt`。
   - nginx：`sbin/nginx` / `modules/*.so` → `$ORIGIN/../lib/phprt`。
3. 按 profile 的 `post_install` 执行动作：`ensure_dir` / `write_file` /
   `copy_ca_bundle` / `write_component_ini`。PHP 的 `php.ini`、`00-ssl.ini`、CA 和
   shared 组件 ini 都在这里生成；动作内容见 [PRODUCT.md](PRODUCT.md)。
4. 运行 profile 的 `verify`；有 `verify_modules` 时再逐个校验组件的 `modules`。
5. 按 profile 的 `entry` / `entry_template` 写项目入口。PHP 写 `myphp-<系列>`；
   项目里只有一个 PHP 系列时再写 `myphp`。nginx 不生成入口。

`myphp-<系列>` 会设置 `PHPRC`、`PHP_INI_SCAN_DIR`、`SSL_CERT_FILE`、`PATH` 和
`extension_dir`，所以整棵树搬走后仍能用；入口里写死安装树目录，不需要
`.phprt/current`。

## 7. 构建工具

工具清单来自 recipe，不写死在 Rust：

| 来源 | 内容 |
| --- | --- |
| 产品 profile 的 `tools` | 这个产品总是需要的命令，可带最低版本，如 `tool("gcc", min = "9")` |
| dep / component 的 `host_tools` | 只有本次构建包含它时才需要的命令，如 cmake、nasm |
| feature 的 `requires.commands` | 选中 feature 时检查的命令 |
| autoconf | 源码构建，catalog 的 `[autoconf]`，作为组件的 `tools`（dep 包，不是宿主机命令） |
| 纯 Perl 模块 | 仓库 `vendor/perl`，构建时通过 `PERL5LIB` 注入 |

检查在下载源码之前执行；`--downloads` 不检查。版本探测和比较见
[PRODUCT.md](PRODUCT.md)。

Fedora 上系统工具可以一次装好：

```bash
sudo dnf install gcc gcc-c++ make cmake nasm binutils pkgconf-pkg-config patchelf tar m4 perl
```

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
其他 PHP 系列的 recipe 使用同一套流程，目前只在 8.0 ~ 8.5 上验收（8.6 未适配）。
