# 给 agent 的说明：怎么往 phprt 里加 PHP 扩展

> 这份文档是给 **AI agent** 读的。人只需要把仓库指给自己的 agent，说一句
> 「按 docs/ADD-EXTENSION.md 加 xxx 扩展」即可。
>
> 约定：所有命令都在 `phprt` 仓库根执行；安装根默认是 `~/.phprt`，
> 本机实测用的是项目内的 `blog/.phprt`（`phprt` 命令会自动从当前目录往上找）。

---

## 1. 先判断属于哪一类

| 类型 | 特征 | 处理方式 | 例子 |
| --- | --- | --- | --- |
| **PECL 扩展** | 独立的 `.so`，用 `phpize` 编译，不改 PHP 本体 | 照 §4 写一个模块，**不用重编 PHP**（~1 分钟） | redis、xdebug、apcu、yaml、swoole、imagick、mongodb |
| **内置扩展** | 必须作为 configure 参数编进 PHP 本体（`--with-*` / `--enable-*`） | 照 §5.2 写 `builtin/<扩展>.sh`，重编 PHP（~3 分钟） | zlib、intl、gd、sqlite、curl、mbstring |
| **依赖库** | 不是 PHP 扩展，只是某个扩展/库需要的 C 库 | 照 §5.1 / §5.3 写 `deps/<库>.sh`，由消费者的 `needs` 拉起 | gmp、libpq、sodium、tidy |

判断依据：PHP 源目录里 `ext/<名字>/config.m4` 能不能单独 `phpize`。能独立编的走 PECL（哪怕它在 PHP 源码里，也能用 `phpize` 编）；只能改 `--with-*` 的走 `builtin/`；两者都不是、只是 C 库的走 `deps/`。

---

## 2. 目录与关键概念

```
phprt/
├── configs/php-<版本>.conf   一个版本一份：版本 + 开关 + 依赖版本（用户改这里）
├── lib/common.sh            路径、下载、stamp、dep_dir_of、enabled_dep_names、php_sig
├── lib/modules/deps/*.sh    独立库（安装到 <安装根>/deps/）
├── lib/modules/ext/*.sh     PECL 扩展（安装到 php-<版本>/）
├── lib/modules/builtin/*.sh PHP 内置扩展：开关 + configure 参数 + 依赖（php.sh 收集）
├── lib/modules/tools/*.sh   构建工具（autoconf / re2c / perl 核心模块）
├── lib/modules/php.sh       PHP 本体的 configure 参数
├── lib/verify.sh            自检（扩展有没有真的加载）
├── lib/myphp-project.tmpl   项目入口模板（不用动）
└── docs/ADD-EXTENSION.md    本文件

<安装根>/
├── php-<版本>/               PHP 本体
├── deps/<名字>-<版本>/        依赖库（lib 或 lib64）
├── downloads/               下载缓存（离线安装靠它）
├── logs/<run>/              每次构建的日志
└── php<版本>_{default,dev,prod}.ini
```

- **目录即类别**：`deps/` 产库、`ext/` 产 PECL 扩展、`builtin/` 只是内置扩展的
  声明（不产 `.so`，由 `php.sh` 收集参数）、`tools/` 产构建工具。
- **一个 `ENABLE_*` 开关必须恰好被一个文件认领**，`phprt verify` 的 5d 会检查：
  有开关没文件、或有文件没开关都会报错。归属关系用 `phprt modules` 看。
- **硬依赖写在模块头部**：`#  phprt: needs=zlib`（逗号分隔，写依赖名，不写 `libz`）。
  缺库时 `ensure_needs` 会先构建它，`dep_env` 自动注入 `-I` / `-L` / `-rpath` /
  `PKG_CONFIG_PATH`。只有「上游没给关闭或指路参数」的库才登记
  （例：swoole 的 `AC_CHECK_LIB(z)` 没有 `--with-zlib-dir`）。
- **stamp**：`stamp_write <模块> <签名>` 把签名写进被描述目录里的 `.phprt-stamp-<模块>`。
  签名一致 → 跳过；目录被删 → 自动重装。签名里通常带上依赖版本和 `$(php_sig)`
  （PHP 一重编，PECL 扩展的 ABI 就变了，必须跟着重编）。
- **`--force`** 只清该模块自己的目录；**`--rebuild`** 复用解压好的源码重编译。

---

## 3. 需要同步修改的地方（清单）

先确定要加的是哪一类；三类改法不同。

### 3.1 加一个 PECL 扩展（要下载源码、`phpize`）

| # | 文件 | 改什么 |
| --- | --- | --- |
| 1 | `config.conf` | 开关 `ENABLE_X=1` + `X_VERSION/URL/SHA256` + 行尾归属注释 |
| 2 | `lib/modules/ext/x.sh` | 新建模块脚本（头部 `#  phprt: switch=ENABLE_X needs=...`） |
| 3 | `build.sh` → `EXT_MODULES` | 加模块名 |
| 4 | `build.sh` → `enabled_module()` | 开关 → 模块名 |
| 5 | `lib/common.sh` → `stamp_file()` | 新名字放进 `php\|redis\|...` 那行 |
| 6 | `build.sh` → `--only php` 补充列表 | PHP 重编后跟着重编 |
| 7 | `lib/verify.sh` → `check_ext` | 自检确认真的加载 |
| 8 | `README.md` 扩展清单 | 记账 |

### 3.2 加一个内置扩展（PHP 源码自带，编进 php 本体）

| # | 文件 | 改什么 |
| --- | --- | --- |
| 1 | `config.conf` | 开关 `ENABLE_X=1` + 行尾归属注释 |
| 2 | `lib/modules/builtin/x.sh` | 开关 + 正负向 configure 参数 + `--needs` |
| 3 | `lib/common.sh` → `builtin_order()` | 加进固定顺序（只影响参数顺序） |
| 4 | `lib/verify.sh` → `check_ext` | 自检确认真的加载 |
| 5 | `README.md` | 记账 |

**不用改**：`php.sh`（只收集参数）、`php_sig()`（内置文件哈希自动进签名）、
`enabled_dep_names()`（从 `--needs` 派生）。

### 3.3 加一个依赖库

| # | 文件 | 改什么 |
| --- | --- | --- |
| 1 | `config.conf` | 版本 / URL / SHA256（有开关就写开关；没有就由消费者的 `needs` 拉起） |
| 2 | `lib/modules/deps/x.sh` | 新建模块脚本；它自己的依赖写 `#  phprt: needs=` |
| 3 | `build.sh` → `LIB_MODULES` | 加名字（必须排在 `php` 之前） |
| 4 | `lib/common.sh` → `dep_dir_of()` | 依赖目录名 |
| 5 | `build.sh` → `enabled_module()` | 只有“有开关的库”才需要 |
| 6 | `lib/verify.sh` | 5c 自动检查（头部有 `needs=` 就会被扫到） |

> 少改 5 会导致「PHP 变了但扩展还是旧 ABI」；少改 3.1 的第 6 项会导致
> `--only php` 之后扩展消失。

---

## 4. 模板 A：PECL 扩展（以 msgpack 为例）

### 4.1 仓库的 `config.conf`（模板）

```conf
ENABLE_MSGPACK=1
MSGPACK_VERSION=3.0.1
MSGPACK_URL="https://pecl.php.net/get/msgpack-3.0.1.tgz"
MSGPACK_SHA256=""
```

### 4.2 `lib/modules/ext/msgpack.sh`

```bash
#!/usr/bin/env bash
# msgpack 扩展（PECL + phpize）
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

need_cmds gcc make tar
PHP="$PREFIX/bin/php"
[ -x "$PHP" ] || die "还没有 PHP：先 ./build.sh --only php"

AC_DIR="$(dep_dir_of autoconf)"      # phpize 需要 autoconf
PERL_DIR="$(dep_dir_of perl5core)"   # 精简系统缺 perl 核心模块
SIG="msgpack=$MSGPACK_VERSION;$(php_sig)"

INI="$PREFIX/etc/conf.d/24-msgpack.ini"
if [ "$FORCE" != 1 ] && stamp_ok msgpack "$SIG" && [ -f "$INI" ]; then
  ok "msgpack 已装，跳过"; exit 0
fi

TAR="$(basename "$MSGPACK_URL")"     # 缓存文件名 = URL 最后一段
fetch "$MSGPACK_URL" "$DOWNLOADS/$TAR" "${MSGPACK_SHA256:-}"

SRC="$SRCDIR/msgpack-$MSGPACK_VERSION-php-$PHP_VERSION"
rm -rf "$SRC"
log "解压 $TAR"
extract "$DOWNLOADS/$TAR" "$SRC" 1   # 1 = 去掉 PECL 包顶层目录

# phpize 的前置工具
if [ ! -x "$AC_DIR/bin/autoconf" ]; then bash "$MODULES/autoconf.sh"; fi
if [ ! -f "$PERL_DIR/share/perl5core/IPC/Cmd.pm" ]; then bash "$MODULES/perlshim.sh"; fi

LIB_DIRS="$(dep_lib_dirs $(enabled_dep_names) | tr '\n' ':' | sed 's/:$//')"
export PATH="$AC_DIR/bin:$PREFIX/bin:$PATH"
export LD_LIBRARY_PATH="$PREFIX/lib${LIB_DIRS:+:$LIB_DIRS}${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export PHP_AUTOCONF="$AC_DIR/bin/autoconf" PHP_AUTOHEADER="$AC_DIR/bin/autoheader"
export PERL5LIB="$PERL_DIR/share/perl5core${PERL5LIB:+:$PERL5LIB}"

cd "$SRC"
log "phpize"
phpize > "$LOGS/msgpack-phpize.log" 2>&1 \
  || { tail -30 "$LOGS/msgpack-phpize.log"; die "phpize 失败"; }

log "configure"
./configure --with-php-config="$PREFIX/bin/php-config" \
  > "$LOGS/msgpack-configure.log" 2>&1 \
  || { tail -30 "$LOGS/msgpack-configure.log"; die "configure 失败"; }

log "make -j$JOBS"
make -j"$JOBS" > "$LOGS/msgpack-make.log" 2>&1 \
  || { tail -40 "$LOGS/msgpack-make.log"; die "make 失败"; }
make install >> "$LOGS/msgpack-make.log" 2>&1

EXT_DIR="$("$PREFIX/bin/php-config" --extension-dir)"
[ -f "$EXT_DIR/msgpack.so" ] || die "没找到 $EXT_DIR/msgpack.so"

# ini 编号约定：20-redis / 23-protobuf / 24-msgpack / 25-apcu / 26-yaml / 27-swoole / 30-xdebug
cat > "$INI" <<EOF
; 由 phprt 生成（msgpack $MSGPACK_VERSION）
extension=msgpack.so
EOF

stamp_write msgpack "$SIG"
ok "msgpack $MSGPACK_VERSION -> $EXT_DIR/msgpack.so"
```

### 4.3 其余改动

```
build.sh   ALL_MODULES=(... msgpack ...)          # 放在 php 之后
           enabled_module: msgpack) [ "${ENABLE_MSGPACK:-0}" = 1 ] ;;
           --only php 的追加列表: [ "${ENABLE_MSGPACK:-0}" = 1 ] && MODULES_TO_RUN+=(msgpack)
lib/common.sh  stamp_file: php|redis|xdebug|apcu|yaml|swoole|msgpack) ...
lib/verify.sh  check_ext msgpack "${ENABLE_MSGPACK:-0}"
```

### 4.4 需要依赖库时

如果是 `imagick`（要 ImageMagick）、`memcached`（要 libmemcached）、`yaml`（要 libyaml）这类，
先在配置里加依赖库版本，写一个依赖模块（见 §5.1），再在 `configure` 里加
`--with-xxx="$DEP"`，并在 `LD_LIBRARY_PATH` 里带上它。

---

## 5. 模板 B：编译期扩展 + 依赖库（以 gmp 为例）

### 5.1 依赖库模块 `lib/modules/deps/gmp.sh`（autotools 版）

```bash
#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

need_cmds gcc make tar

D="$(dep_dir_of gmp)"
SIG="gmp=$GMP_VERSION"

if [ "$FORCE" != 1 ] && stamp_ok gmp "$SIG" && [ -f "$D/lib/libgmp.so" ]; then
  ok "gmp 已安装，跳过"; exit 0
fi

ensure_clean_target gmp "$D"

TAR="$(basename "$GMP_URL")"
fetch "$GMP_URL" "$DOWNLOADS/$TAR" "${GMP_SHA256:-}"

SRC="$SRCDIR/gmp-$GMP_VERSION"
rm -rf "$SRC"
extract "$DOWNLOADS/$TAR" "$SRCDIR"

cd "$SRC"
log "configure + make -j$JOBS"
./configure --prefix="$D" --libdir="$D/lib" --disable-static \
  > "$LOGS/gmp-configure.log" 2>&1 \
  || { tail -20 "$LOGS/gmp-configure.log"; die "gmp configure 失败"; }
make -j"$JOBS" > "$LOGS/gmp-make.log" 2>&1 \
  || { tail -20 "$LOGS/gmp-make.log"; die "gmp make 失败"; }
make install >> "$LOGS/gmp-make.log" 2>&1

stamp_write gmp "$SIG"
ok "gmp $GMP_VERSION -> $D"
```

**cmake 工程的骨架**（libwebp/libaom/libavif 都是这个套路，照抄改参数即可）：

```bash
cmake -S "$SRC" -B "$SRC/build" \
  -DCMAKE_INSTALL_PREFIX="$D" \
  -DCMAKE_INSTALL_LIBDIR=lib \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
  -DBUILD_SHARED_LIBS=ON \
  > "$LOGS/xxx-cmake.log" 2>&1 || { tail -30 "$LOGS/xxx-cmake.log"; die "cmake 失败"; }
cmake --build "$SRC/build" --parallel "$JOBS" > "$LOGS/xxx-make.log" 2>&1 || { ...; die; }
cmake --install "$SRC/build" >> "$LOGS/xxx-make.log" 2>&1
```

### 5.2 内置扩展：`lib/modules/builtin/x.sh`

内置扩展没有独立源码包，只需声明「开关 → configure 参数 → 依赖」：

```bash
#!/usr/bin/env bash
#  PHP 内置扩展：xml（以 libxml2 为例）
#  phprt: builtin=xml switch=ENABLE_XML
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../../common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_XML:-0}" = 1 ]; then
      printf '%s\n' "--with-libxml=$(dep_dir_of libxml2)"
    else
      printf '%s\n' --without-libxml      # 关掉时也要给负向参数，否则会探测系统库
    fi ;;
  --needs)
    if [ "${ENABLE_XML:-0}" = 1 ]; then printf '%s\n' libxml2; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
```

加完再往 `builtin_order()` 里补一个名字（决定 configure 参数顺序），
`php.sh` 完全不用动；改这个文件会自动触发 PHP 重编。

### 5.3 `lib/common.sh` / `build.sh`（依赖库）

```bash
dep_dir_of()     { ... gmp) dep_dir gmp "$GMP_VERSION" ;; ... }
# build.sh
LIB_MODULES=(... gmp ...)                 # 必须排在 php 之前
enabled_module() { ... gmp) [ "${ENABLE_GMP:-0}" = 1 ] ;; ... }   # 有开关才需要
```

`enabled_dep_names()` **不用改**：它现在从各节点的 `needs` / `--needs` 派生，
消费者（`builtin/*.sh`、`deps/*.sh`、`ext/*.sh`）谁声明了 `needs=gmp` 就自动进
rpath 和 `LD_LIBRARY_PATH`。`php_sig()` 也不用再为内置扩展手工加一项
（文件的代码指纹已经进签名）。

---

## 6. 构建与验证

```bash
cd <安装根所在的项目>          # 例如 blog/
phprt build --only libgmp     # 先单独验证依赖库能编过
phprt build --only php        # 编译期扩展：重编 PHP（会自动带上 PECL 扩展）
phprt build --only msgpack    # PECL 扩展
phprt build                   # 全量增量（推荐最后跑一次）
phprt verify                  # 自检：加载情况 + 动态库 + 搬迁测试
```

验证单个扩展：

```bash
./myphp -r 'var_dump(extension_loaded("msgpack"), phpversion("msgpack"));'
./myphp -m | grep -i msgpack
./myphp --ini                 # 看 ini 从哪加载
```

离线构建：先把源码包放进 `phprt/downloads/`（`install.sh` 会硬链到安装根），
然后 `phprt build --offline`。

---

## 7. 踩过的坑（重要）

1. **`lib64` 陷阱**：cmake/autotools 在 64 位系统上可能装到 `<dep>/lib64`。
   `dep_lib_dirs()` 已经同时认 `lib` 和 `lib64`，但**自己写 cmake 参数时最好显式
   `-DCMAKE_INSTALL_LIBDIR=lib`**（`libjpeg-turbo` 就因为这个悄悄链到了系统库）。
2. **汇编器**：`libaom` 没有 `nasm`/`yasm` 会直接 configure 失败
   （加 `-DAOM_TARGET_CPU=generic`）；`libjpeg-turbo` 没 nasm 要 `-DWITH_SIMD=OFF`。
3. **CMake 4**：新 CMake 不再兼容 `cmake_minimum_required(<3.5)` 的老项目，
   报错就换老版本 cmake 或给项目打补丁。
4. **PHP 的 `--with-xxx` 找不到库**：把依赖的 `lib/pkgconfig` 放进 `PKG_CONFIG_PATH`，
   或显式 `--with-xxx="$DEP"`。
5. **PECL 扩展必须带 `$(php_sig)`**：否则 PHP 重编后 ABI 不匹配，加载时报
   `undefined symbol`。
6. **`zend_extension` vs `extension`**：xdebug/opcache 这类要用 `zend_extension=`。
7. **ini 加载顺序**：`conf.d/` 按文件名排序，编号留间隔（20/24/25/26/27/30）。
8. **下载中断**：`fetch()` 先写 `.part` 再原子改名，**不要手工改回去**；
   半截文件会被当成缓存命中，解压时才炸。
9. **GitHub 需要代理**：`fetch()` 直连失败会自动走 `http://127.0.0.1:10808`
   （`PHPRT_PROXY=` 可改）；`curl` 手工下时加 `-x`。
10. **`--force` 的语义**：只清「这个模块自己的目录」，不会动别的模块；
    但会连带 `--rebuild`（复用源码重编）。
11. **PHP 8.6 移除了 `zval_dtor()`**（8.3 起弃用）：老 PECL 扩展会报
    `implicit declaration of function 'zval_dtor'`。等价替换成 `zval_ptr_dtor()` 即可
    （`lib/modules/ext/protobuf.sh` 里有现成的幂等补丁）。
12. **扩展需要的库要自己指向安装根**：上游 configure 若用裸 `AC_CHECK_LIB(x)`
    （没有 `--with-x-dir`），`LD_LIBRARY_PATH` 对链接器无效，会悄悄链上系统库。
    在模块头写 `#  phprt: needs=x` 并 `dep_env` 注入 `-L`/`-rpath`；
    改完记得在签名里加个标记（如 `;rpath=1`），否则 stamp 命中会跳过重编。
13. **内置扩展关闭时必须给负向参数**：`--without-zlib` / `--without-openssl` 这类
    不能省。PHP 的 configure 在两者都不给时会自动探测系统库，悄悄链上系统的。
    默认就关闭的扩展（mbstring/intl/zip/ffi）可以只写正向参数。
14. **改 `builtin/*.sh` 会自动重编 PHP**：`php_sig()` 里带了这些文件的代码指纹；
    不需要手动改签名。

---

## 8. 当前没编、但常见的（可以直接照上面加）

| 扩展 | 需要的外部库 |
| --- | --- |
| `imagick` | ImageMagick（顺带能解决 HEIC/HEIF） |
| `pgsql` / `pdo_pgsql` | libpq（postgres 源码里的 `src/interfaces/libpq`） |
| `memcached` | libmemcached + libevent |
| `amqp` | rabbitmq-c |
| `ssh2` | libssh2 |
| `maxminddb` | libmaxminddb |
| `uuid` | util-linux 的 libuuid |
| `zstd` | libzstd |
| `bz2` | bzip2（没有 configure，得手写 Makefile 调用） |
| `oci8` / `pdo_oci` | Oracle Instant Client（闭源，必须手工下载） |
| `ldap` | openldap |
| `snmp` | net-snmp |

已经在用的清单见 `README.md` 的「扩展」一节。
