# `--static-all`：单个 PHP 可执行文件

本文定义 `phprt install --static-all` 的行为。它是 `--static` 的加强版：
**扩展和第三方依赖都链接进 `bin/php`**，产物可以单独复制到别的机器运行。

## 1. 目标与非目标

目标：

- 选中的扩展全部编入 PHP，不生成 `.so`。
- 扩展依赖的 C 库全部用静态档案（`.a`）链接。
- `bin/php` 不依赖安装树里的 `deps/` 或 `lib/phprt/`。
- 保留正常的 `phprt verify` 和增量构建能力。

非目标：

- **不静态链接 glibc。** `ldd bin/php` 仍会看到 `libc.so.6`、`libm.so.6`
  这类系统库，这是有意保留的。
- 不改变 `--static` 的现有含义。`--static` 仍是“扩展静态、依赖动态”。
- 不保证所有 recipe 立刻支持；支持范围按第 5 节逐步扩大。

## 2. 与 `--static` 的区别

| 选项 | 扩展 | 依赖 | 产物 |
| --- | --- | --- | --- |
| `--static` | 支持 static 的编入 PHP | 仍用 `.so` | 依赖 `deps/` 或 `lib/phprt/` |
| `--static-all` | 支持 static 的编入 PHP | 全部用 `.a` | 单个 `bin/php` |

`--static-all` 遇到 `type=shared` 的扩展（如 `xdebug`、`swoole`）时**自动排除**，
不再像 `--static` 那样保留为 `.so`。需要 PHP 和依赖都开 LTO 时用
`--static-all-lto`，见 [STATIC-ALL-LTO.md](STATIC-ALL-LTO.md)。

## 3. 用法

```bash
# 最小版本：现有依赖已经都有 .a
phprt install 8.5 --static-all --ext "openssl zlib curl sqlite3 mbstring"

# 只看计划，不构建
phprt install 8.5 --static-all --ext "openssl zlib curl sqlite3 mbstring" --plan

# 切回普通（依赖动态）安装
phprt install 8.5 --shared --ext "openssl zlib curl sqlite3 mbstring"
```

规则：

- `--static-all` 只作用于 `--ext` 选中的扩展；不写 `--ext` 时用
  `bundle.conf` 的 `DEFAULT_EXTS`。
- 选中集合里 `type` 不含 `static` 的扩展会被排除并提示。
- 与 `--shared` 互斥。
- 与 `--build` 同时使用时，只接受 `名字=static`；出现 `名字=shared` 直接报错，
  避免单文件里混入 `.so`。
- 记录在 `$TREE/.phprt-state/link`：
  - `static`：`--static-all` 安装。
  - `shared`：普通安装。
- `install` 不带 `--static-all` / `--shared` 时沿用已记录的方式。
- `phprt show` 显示当前链接方式。

## 4. 构建机制

### 4.1 依赖

静态模式下：

- 依赖库必须提供 `.a`；
- PHP 的链接参数不再写依赖 rpath，只保留 `-L`；
- 允许依赖同时保留 `.so`。它们不参与 `bin/php` 的链接，复制 `bin/php`
  时也不需要它们。

依赖没有 `.a` 时构建失败，并指出缺的是哪个库。补齐依赖静态库属于第 5 节的增量工作。

### 4.2 PHP 链接

`php.sh` 在静态模式下做四件事：

1. 在 `$BUILD_ROOT/static-libs/` 建一个只含依赖 `.a` 软链的目录，并把它
   插到 `EXTRA_LDFLAGS` / `EXTRA_LDFLAGS_PROGRAM` 的最前面。GNU ld 在第一个
   有同名库的目录里选定 `libfoo.a`，于是 `-lfoo` 不会退化成 `.so`。
2. 删掉 configure 自动加的 `-Wl,-rpath,<deps>` 和 `-R <deps>`。
3. 把 `EXTRA_LIBS` 重复两遍，并把 `static_ldflags()` 的输出放在每一遍末尾。
   静态库是单向扫描：`libcurl.a` 在 `libssl.a` 之前时，第一遍拉不出后面的
   符号，第二遍再扫一次就能解决。
4. 加 `-static-libgcc` 和 `-static-libstdc++`，避免引入项目外的
   `libgcc_s.so` / `libstdc++.so`。

不使用 `-Wl,-Bstatic`：libtool 会把它和 `--start-group` 重排到对象文件之前，
实际链接时这些库仍然是动态的。

PHP 签名包含链接方式：`static` ↔ `shared` 切换会重编 PHP。

### 4.3 扩展

- `builtin`：沿用 `flags()`，源码直接编入 PHP。
- `ext`：沿用 `stage_static()`，源码放进 `php-src/ext/<名字>`。
- 静态构建成功后清掉旧的 `.so` 和 `extension=` 行；设置项 ini 保留。

## 5. 支持范围

| 阶段 | 范围 | 状态 |
| --- | --- | --- |
| v0 | `openssl zlib curl sqlite3 mbstring` | 已实现（2026-11-09） |
| LTO | `--static-all-lto`：PHP + `openssl zlib curl sqlite3 oniguruma` | v0 已实现（2026-11-09） |
| v1 | `libxml2` 已提供 `.a`；`libzip`、`libsodium` 待做 | 部分完成（2026-11-09） |
| v2 | PECL：redis、yaml、ssh2、apcu、msgpack、igbinary、protobuf、amqp、memcached、mongodb、rdkafka | 待做 |
| v3 | intl/ICU、gd 图片链、xsl、ldap、pgsql、gmp、ffi、tidy | 待做 |
| v4 | imagick | 待做 |
| 最后 | glibc 全静态实验（`-static`） | 可选 |

v0 选择 `openssl zlib curl sqlite3 mbstring` 的原因：这些依赖当前已经产出 `.a`，
可以先验证 CLI、链接和验收流程，不需要改任何 `deps/` recipe。

默认集 `openssl zlib libxml2 curl sqlite3 mysqli pdo_mysql mbstring zip sodium gettext`
在 v1 完成后才能全部使用 `--static-all`。

## 6. 验收

```bash
cd /path/to/project
phprt install 8.5 --static-all --ext "openssl zlib curl sqlite3 mbstring"
phprt show
phprt verify
```

检查 `bin/php`：

```bash
PHP=.phprt/php-8.5.12/bin/php

# 只允许 glibc 等基础库
ldd "$PHP"

# 不能有指向 deps/ 的 RPATH/RUNPATH
readelf -d "$PHP" | grep -E 'RPATH|RUNPATH|NEEDED'

# 不能有共享扩展
ls .phprt/php-8.5.12/lib/php/extensions 2>/dev/null
```

单文件验证：

```bash
mkdir -p /tmp/php-onefile
cp .phprt/php-8.5.12/bin/php /tmp/php-onefile/php
cd /tmp/php-onefile

./php -v
./php -m
./php -r 'var_dump(extension_loaded("openssl"), extension_loaded("zlib"),
                  extension_loaded("curl"), extension_loaded("sqlite3"),
                  extension_loaded("mbstring"));'
./php -r 'var_dump(Phar::canCompress(Phar::GZ));'
```

通过标准：

- `ldd` 只列出 glibc 基础库和动态加载器；
- `readelf -d` 没有 `deps/` 路径；
- `php -m` 包含选中的全部扩展；
- `Phar::canCompress(Phar::GZ)` 为 `true`；
- 在空目录运行不报缺少共享库。

## 7. 已知限制

- v0 只覆盖第 5 节列出的五个扩展；其它扩展需要先补依赖的 `.a`。
- `--static-all` 的树不能再 `add ext --shared`，因为不存在可用的依赖 `.so` 链接路径。
- `add ext --static` 在静态树里可用，但对应依赖必须有静态库。
- 不静态 glibc：DNS/NSS、locale、用户查询仍走系统 glibc；全静态的
  可行性和发布限制见 [STATIC-LIBC.md](STATIC-LIBC.md)。
- 依赖 `.so` 可能仍留在 `deps/`，但不会出现在 `bin/php` 的 `NEEDED` 里；
  复制时只需要 `bin/php`。
