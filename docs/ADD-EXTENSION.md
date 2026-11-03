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
| **编译期扩展** | 必须作为 configure 参数编进 PHP 本体，或依赖某个 C 库 | 照 §5：写依赖库模块 + 改 `php.sh` + 重编 PHP（~3 分钟） | intl、gd、pgsql、gmp、sodium、xsl、tidy |

判断依据：PHP 源目录里 `ext/<名字>/config.m4` 能不能单独 `phpize`。能独立编的都能走 PECL 路线（哪怕它在 PHP 源码里，也能用 `phpize` 编）；必须改 `--with-*` 的只能走编译期。

---

## 2. 目录与关键概念

```
phprt/
├── config.conf              开关 + 依赖版本（用户改这里）
├── lib/common.sh            路径、下载、stamp、dep_dir_of、enabled_dep_names、php_sig
├── lib/modules/<名字>.sh     每个「模块」一个脚本（模块名 = 文件名）
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

- **模块名 = `lib/modules/<名字>.sh` 的文件名**，`build.sh` 用它找脚本。
- **stamp**：`stamp_write <模块> <签名>` 把签名写进被描述目录里的 `.phprt-stamp-<模块>`。
  签名一致 → 跳过；目录被删 → 自动重装。签名里通常带上依赖版本和 `$(php_sig)`
  （PHP 一重编，PECL 扩展的 ABI 就变了，必须跟着重编）。
- **`--force`** 只清该模块自己的目录；**`--rebuild`** 复用解压好的源码重编译。

---

## 3. 需要同步修改的地方（清单）

加 **PECL 扩展** 只改 4 处；加 **编译期扩展/依赖库** 还要多改 2 处：

| # | 文件 | 改什么 | PECL | 编译期 |
| --- | --- | --- | :-: | :-: |
| 1 | `config.conf` | 开关 `ENABLE_XXX=1` + `XXX_VERSION/URL/SHA256` | ✅ | ✅ |
| 2 | `lib/modules/<名字>.sh` | 新建模块脚本 | ✅ | ✅ |
| 3 | `build.sh` → `ALL_MODULES` | 加模块名（依赖库要排在 `php` 之前） | ✅ | ✅ |
| 4 | `build.sh` → `enabled_module()` | 开关 → 模块名 | ✅ | ✅ |
| 5 | `lib/common.sh` → `dep_dir_of()` | 依赖库的目录名 | — | ✅ |
| 6 | `lib/common.sh` → `enabled_dep_names()` | 让依赖目录进 `LD_LIBRARY_PATH`/rpath | — | ✅ |
| 7 | `lib/common.sh` → `php_sig()` | 开关变了要触发 PHP 重编 | — | ✅ |
| 8 | `lib/common.sh` → `stamp_file()` | 新 PECL 名放进 `php\|redis\|...` 那行 | ✅ | — |
| 9 | `build.sh` → `--only php` 的补充列表 | PHP 重编后跟着重编的扩展 | ✅ | — |
| 10 | `lib/verify.sh` → `check_ext` | 新增一行，自检会验证真的加载了 | ✅ | ✅ |
| 11 | `README.md` 扩展清单 | 记账 | ✅ | ✅ |

> 少改 8 会导致「PHP 变了但扩展还是旧 ABI」；少改 9 会导致 `--only php` 之后扩展消失。

---

## 4. 模板 A：PECL 扩展（以 msgpack 为例）

### 4.1 `config.conf`

```conf
ENABLE_MSGPACK=1
MSGPACK_VERSION=3.0.1
MSGPACK_URL="https://pecl.php.net/get/msgpack-3.0.1.tgz"
MSGPACK_SHA256=""
```

### 4.2 `lib/modules/msgpack.sh`

```bash
#!/usr/bin/env bash
# msgpack 扩展（PECL + phpize）
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
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

# ini 编号约定：20-redis / 24-msgpack / 25-apcu / 26-yaml / 27-swoole / 30-xdebug
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

### 5.1 依赖库模块 `lib/modules/libgmp.sh`（autotools 版）

```bash
#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$(readlink -f "$0")")/../common.sh"
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

### 5.2 `lib/modules/php.sh` 里加 configure 参数

```bash
D_GMP="$(dep_dir_of gmp)"
[ "${ENABLE_GMP:-0}" = 1 ] && ARGS+=(--with-gmp="$D_GMP")
```

### 5.3 `lib/common.sh` 三处

```bash
dep_dir_of()  { ... gmp) dep_dir gmp "$GMP_VERSION" ;; ... }
enabled_dep_names() { ... [ "${ENABLE_GMP:-0}" = 1 ] && echo gmp ... }
php_sig()     { ... s+=";gmp=${ENABLE_GMP:-0}:$GMP_VERSION" ... }
```

`enabled_dep_names` 的输出会进 PHP 的 rpath 和项目入口的 `LD_LIBRARY_PATH`，
**漏了它**就会出现「编译能过、运行时找不到 so」。

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
