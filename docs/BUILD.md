# 编译与安装

`install` / `update` 在解压之后按构建顺序编译源码，产物安装到
`.phprt/php-<php 版本>/`，最后生成项目入口 `myphp` 和 `.phprt/current`。
`--downloads` 在下载阶段结束，不进入本阶段。

## 1. 顺序

```text
dep → 静态扩展（只贡献 PHP configure 参数） → php → 共享扩展
```

- 拓扑序由求解器决定，构建阶段不再排序。
- 静态扩展不单独执行命令。选中的 static 扩展把 ext recipe 的 `configure` 交给
  PHP 的 `./configure`；没选或选了 shared 的扩展使用 php recipe `builtin_disable`
  里的关闭参数。同一个扩展只出现一组参数。
- 共享扩展在 PHP 之后构建：需要 `php-config`、`phpize` 和 PHP 源码里的扩展目录。
- `tools`（如 autoconf）作为 dep 节点参与顺序，只进 `PATH`，不参与链接。

## 2. 每个节点

| 类型 | 工作目录 | 命令 |
| --- | --- | --- |
| dep | `build/<名字>/<补丁>` | recipe 的 configure + make |
| php | `build/php/<补丁>` | `./configure <全部参数>` + make |
| shared 内置扩展 | `build/<名字>/php-<php 版本>` | 从 PHP 源码 `shared_from` 拷贝 → phpize → configure → make → make install |
| shared PECL | `build/<名字>/<补丁>` | phpize → configure → make → make install |

- 共享扩展的 configure 自动带 `--with-php-config=<tree>/bin/php-config`。
- 编进 PHP 的扩展不写 ini；共享扩展写 `{tree}/etc/conf.d/<ini>`。
- 从共享切到静态时，构建阶段会删掉该扩展的旧 ini，避免重复加载。

## 3. 环境

每次执行命令注入以下变量：

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
| `PHP_AUTOCONF` / `PHP_AUTOHEADER` | 树内 autoconf 的路径 |

构建阶段使用绝对路径；`plan` 的输出仍然用相对项目根的路径，便于阅读。

## 4. 跳过与重编

每个工作目录写 `.phprt-sig`，内容包含：

- 节点名、版本、链接方式；
- configure 程序和全部参数；
- make 命令；
- 注入的环境变量；
- 预期的产物路径。

签名一致且产物齐全时跳过。签名变化时先执行 `make distclean`（忽略失败），
再重新 configure / make。源码目录在 `build/`，删除它即可强制重新开始。

## 5. 日志与失败

- 每个节点一份日志：`build/logs/<kind>-<name>-<version>.log`；PHP 的名字和版本相同，只写一次。
- 命令失败时打印日志末尾 40 行，退出 1，不写 state。
- 已经完成的节点、已解压的源码和已安装的树都会保留，重跑时按签名跳过。

## 6. 运行时整理

所有构建完成后：

1. 把各 dep 的 `lib/*.so*`（含软链）复制到 `{tree}/lib/phprt`。
2. 用 `patchelf --force-rpath` 统一 rpath：
   - `lib/phprt/*` → `$ORIGIN`
   - `{tree}/bin/*` → `$ORIGIN/../lib/phprt`
   - `{tree}/lib/php/extensions/*/*.so` → `$ORIGIN/../../../phprt`
3. 写 `{tree}/etc/php.ini`（不存在时）、shared 扩展的 `conf.d` ini。
4. 复制系统 CA 到 `{tree}/etc/ssl/cert.pem`，写 `conf.d/00-ssl.ini`。
5. 运行 `{tree}/bin/php -v` 和 `php -m`，逐个校验扩展的 `modules`。
6. 写项目根 `myphp`，`.phprt/current` 指向本次安装树。

`myphp` 会设置 `PHPRC`、`PHP_INI_SCAN_DIR`、`SSL_CERT_FILE`、`PATH` 和
`extension_dir`，所以整棵树搬走后仍能用。

## 7. 构建工具

| 工具 | 来源 |
| --- | --- |
| gcc / g++ / make / ar / ranlib / pkg-config / patchelf / m4 / perl | 系统 |
| autoconf | 源码构建，catalog 的 `[autoconf]`，作为 ext 的 `tools` |
| 纯 Perl 模块 | 仓库 `vendor/perl`，构建时通过 `PERL5LIB` 注入 |

Fedora 上系统工具可以一次装好：

```bash
sudo dnf install gcc gcc-c++ make binutils pkgconf-pkg-config patchelf tar m4 perl
```

## 8. 验收

```bash
rm -rf .phprt build

# 8.5 默认 openssl + curl(ssl) + libxml
cargo run -- install 8.5
./myphp -v

# 只装 PHP：禁用三个默认扩展
cargo run -- install 8.5 --disable-default-ext openssl --disable-default-ext curl --disable-default-ext libxml

# 再显式加 swoole / xdebug
cargo run -- install 8.5 --ext swoole --ext xdebug
./myphp -m | grep -E 'openssl|curl|swoole|xdebug'

# curl 动态编译，并启用 SSL / nghttp2
cargo run -- install 8.5 --ext 'curl=link:shared,dep:libcurl[ssl,nghttp2]'

# 只下载，不编译
cargo run -- install 8.5 --downloads --offline
```

依赖库应该从树内加载：

```bash
ldd .phprt/php-8.5.12/bin/php | grep libssl
# .../.phprt/php-8.5.12/lib/phprt/libssl.so.3
```

## 9. 不在范围

LTO、`--static-all`、lock 文件、多 PHP 并行构建、增量链接优化。
其他 PHP 系列的 recipe 使用同一套流程，但只在 8.5 上验收。
