# 模块分层与内置扩展声明（记录）

记录 phprt 把 `lib/modules/` 拆成 `deps/` `ext/` `tools/` `builtin/`，并把「哪个扩展需要哪个库」从注释和散落的判断里下沉到模块文件的经过。

## 起点

这次改动之前，同一轮里还有两个提交：`0fe1c8c`（安装根不再有 `config.conf` 软链）、
`20a35ec`（添加 protobuf 扩展）。本文只记录从模块分层开始的这一段。

## 时间线

| 提交 | 动作 | 规模 |
| --- | --- | --- |
| `0f2c65b` | `refactor: modules 按 deps/ext/tools 分层，依赖声明下沉到模块头` | 28 个文件，+156 / −1084 |
| `43cf9cf` | `feat: verify 按 needs 校验硬依赖；文档同步分层与依赖声明` | 4 个文件，+68 / −21 |
| `67e9a03` | `fix: .gitignore 误伤 lib/modules/{deps,tools}，补回被忽略的模块文件` | 20 个文件，+1048 / −6 |
| `60cc380` | `feat: PHP 内置扩展改为 builtin/ 声明文件驱动` | 24 个文件，+480 / −130 |
| `bb58dac` | `test/docs: verify 增加开关归属检查（5d），文档同步内置扩展分类` | 3 个文件，+115 / −29 |

## 起因

`lib/verify.sh` 的 `ldd` 白名单里有一行：

```bash
| # libz 也在白名单里：swoole 无条件链系统 zlib（ABI 稳定，任何发行版都有）
grep -Ev '/(libc|...|libz)\.so'
```

查二进制后发现这句话不准：

```
readelf -d swoole.so | grep NEEDED     -> 只有 libz.so.1（soname，没有路径）
nm -D -u swoole.so | grep deflate      -> deflate / inflate 等符号
readelf -d swoole.so | grep RUNPATH    -> 只有 deps/openssl-3.5.9/lib，没有 zlib
```

- `LD_LIBRARY_PATH=deps/*/lib ldd swoole.so` → `deps/zlib-1.3.2/lib/libz.so.1`（运行时用的是自编库）
- 不带 `LD_LIBRARY_PATH` → `/lib64/libz.so.1`，而这台 Fedora 43 上是 **zlib-ng 的 compat 层**：
  `rpm -qf` 是 `zlib-ng-compat-2.3.3`，`zlibVersion()` 返回 `1.3.1.zlib-ng`，和自编的 `1.3.2` 不是同一实现。

同时「PHP 的某个内置扩展用哪个库」写在了四个地方：`php.sh` 的 `ARGS`、
`lib/common.sh` 的 `php_sig()`、`enabled_dep_names()`、`build.sh` 的 `enabled_module()`。
漏改后两处的后果是「改了开关 PHP 不重编」「rpath 缺库」，`docs/ADD-EXTENSION.md` 里已经标成坑。

## 定下来的规则

四类节点，一个不变量。

| 类别 | 目录 | 产物 | 有没有开关 |
| --- | --- | --- | --- |
| 独立库 | `lib/modules/deps/` | `<安装根>/deps/<名字>-<版本>/` | 部分有（webp / avif） |
| PECL 扩展 | `lib/modules/ext/` | `<安装根>/php-<版本>/`（`.so` + `conf.d/*.ini`） | 有 |
| PHP 内置扩展 | `lib/modules/builtin/` | 不是一个模块，只给 `php.sh` 提供 configure 参数 | 有 |
| 构建工具 | `lib/modules/tools/` | `<安装根>/tools/` | 没有（按需拉起） |

> 不变量：`config.conf` 里每个 `ENABLE_*` 必须恰好被一个文件认领；该文件同时声明它的外部库需求。

依赖关系有三种写法：

| 依赖性质 | 写法 | 例子 |
| --- | --- | --- |
| 无条件硬依赖 | 模块头 `#  phprt: needs=` 或 builtin 的 `--needs` | `ext/swoole.sh`、`builtin/intl.sh` |
| 可选依赖（开关控制） | 模块内部 `ENABLE_*` 判断 | libxml2 / libzip / curl 的 zlib |
| 构建工具 | `[ ! -f ... ] && bash "$(module_sh autoconf)"` | autoconf、re2c、perlshim |

## 一、目录与命名（`0f2c65b`）

```
lib/modules/
├── deps/     openssl zlib libxml2 sqlite curl oniguruma libzip icu
│             libpng jpeg freetype webp aom avif libffi libyaml
├── ext/      redis protobuf xdebug apcu yaml swoole
├── tools/    autoconf re2c perlshim
├── php.sh
└── composer.sh
```

四个模块改名，让「模块名 = 依赖名 = `deps/` 目录前缀」：

```
lib/modules/libcurl.sh -> lib/modules/deps/curl.sh
lib/modules/libwebp.sh -> lib/modules/deps/webp.sh
lib/modules/libaom.sh  -> lib/modules/deps/aom.sh
lib/modules/libavif.sh -> lib/modules/deps/avif.sh
```

`--only libcurl` 这类旧写法在 `build.sh` 里保留了一个 4 项别名。

移动后每个文件头部注释里「可单独执行」的路径、以及 `source .../common.sh` 的相对层级都改了
（`../common.sh` → `../../common.sh`），前者在 `60cc380` 里确认过是 `../../` 而不是 `../../../`。

`common.sh` 新增：

```bash
MODULES_DEPS / MODULES_EXT / MODULES_TOOLS / MODULES_BUILTIN

module_sh() {           # 按名字在 ext/deps/tools 里找脚本，builtin 不参与
  for d in "$MODULES_EXT" "$MODULES_DEPS" "$MODULES_TOOLS" "$MODULES"; do
    [ -f "$d/$1.sh" ] && { printf '%s\n' "$d/$1.sh"; return 0; }
  done
  die "没有模块 $1"
}
module_needs() { ... }  # 读头部 needs=；php 特判为所有已启用内置扩展的 --needs
dep_build_flags() { ... }  # -L<dep>/lib -Wl,-rpath,<dep>/lib
dep_env() { ... }          # 把 CPPFLAGS / LDFLAGS / PKG_CONFIG_PATH 放进环境
ensure_needs() { ... }     # needs 里缺的库先构建（perlshim 同款）
```

`build.sh` 的调度改成 `LIB_MODULES` + `EXT_MODULES` 组装，并按 `needs` 做传递闭包：
`ENABLE_ZLIB=0` 但 `ENABLE_SWOOLE=1` 时，zlib 仍会被构建，而 PHP 的 zlib 扩展保持关闭。

## 二、硬依赖（`0f2c65b` / `43cf9cf`）

首轮登记了 6 个模块：

| 模块 | needs |
| --- | --- |
| `ext/swoole.sh` | `openssl,zlib` |
| `ext/xdebug.sh` | `zlib` |
| `ext/yaml.sh` | `libyaml` |
| `deps/libpng.sh` | `zlib` |
| `deps/freetype.sh` | `zlib,libpng` |
| `deps/libavif.sh` | `aom` |

libxml2 / libzip / curl 的 zlib 仍是可选依赖，走 `ENABLE_ZLIB` 判断，不登记。

模块里改用的写法：

```bash
ensure_needs swoole      # 缺哪个库就先 bash "$(module_sh <库>)"
dep_env swoole           # CPPFLAGS / LDFLAGS / PKG_CONFIG_PATH
```

libpng / freetype / libavif / swoole / xdebug 的签名加了 `;rpath=1`，用来让 stamp 失效、触发一次重编；
否则改完编译参数会被「已安装，跳过」挡住。

## 三、内置扩展 `builtin/`（`60cc380`）

PHP 内置扩展没有独立源码包。PHP 8.5 的 `ext/zlib/config0.m4` 是：

```m4
PHP_ARG_WITH([zlib], [for ZLIB support], [AS_HELP_STRING([--with-zlib], ...)])
PHP_NEW_EXTENSION([zlib], [zlib.c ...], [$ext_shared],, ...)
```

`PHP_ARG_WITH` 只有 3 个参数，`$ext_shared` 永远是空 —— 所以 zlib / openssl 这类**只能静态编进
php 二进制**，不会有 `zlib.so`，`--with-zlib=shared` 也不成立。于是 `builtin/` 里放的不是模块，
而是「开关 + configure 参数 + 依赖」的声明：

```
lib/modules/builtin/
├── openssl.sh  zlib.sh  libxml2.sh  curl.sh  sqlite.sh  mysqli.sh
├── mbstring.sh intl.sh  zip.sh      gd.sh    ffi.sh
```

每个文件两个出口：

```bash
# lib/modules/builtin/zlib.sh
#  phprt: builtin=zlib switch=ENABLE_ZLIB
--flags  -> --with-zlib=$(dep_dir_of zlib)   |   --without-zlib
--needs  -> zlib（仅开关为 1 时）
```

gd 的条件依赖用动态 `--needs`：

```
--needs -> libpng jpeg freetype [webp] [aom avif]
--flags -> --enable-gd --with-jpeg=... --with-freetype=... [--with-webp=...] [--with-avif=...]
```

`php.sh` 里 60 行 if 换成收集循环，`D_OPENSSL` 等 15 个变量删掉：

```bash
for e in $(builtin_order); do
  f="$(builtin_sh "$e")"; [ -n "$f" ] || continue
  while IFS= read -r a; do ARGS+=("$a"); done < <(bash "$f" --flags)
done
```

`builtin_order()` 固定为 `openssl zlib libxml2 curl sqlite mysqli mbstring intl zip gd ffi`，
和原来 `php.sh` 的顺序一致，所以 Configure Command 不会因为这次改动而变。

签名里加了声明文件的代码指纹（忽略注释）：

```bash
s+=";builtin=$(builtin_code_sig)"
# builtin_code_sig: grep -hv '^\s*#' lib/modules/builtin/*.sh | sha256sum | cut -c1-16
```

改 `builtin/*.sh` 会自动让 PHP 重编，不用再手工维护 `php_sig()`。

同时 `ENABLE_ICU` 折进 `builtin/intl.sh --needs -> icu`：ICU 只是 intl 的库，不是功能开关。
开关从 21 个减到 20 个，`php_sig` 里的 `icu=` 只留版本号。

`enabled_dep_names()` 从手写清单改成派生：

```bash
enabled_builtin_needs                      # 内置扩展 --needs
+ 已启用 PECL 扩展的 needs=                # redis/protobuf/xdebug/apcu/yaml/swoole
+ ENABLE_WEBP / ENABLE_AVIF 对应的库
| tr ' ' '\n' | awk 'NF && !seen[$0]++'
```

## 四、自检（`43cf9cf` / `bb58dac`）

- `5c) 硬依赖`：对每个头部有 `phprt: ... needs=` 的模块，**不带 `LD_LIBRARY_PATH`** 跑 `ldd`，
  依赖必须解析到安装根以内；同时检查依赖目录存在。
- `5d) 开关归属`：扫 `builtin/*.sh`、`ext/*.sh`、`deps/*.sh`、`composer.sh` 头部的 `switch=`，
  与 config 里的 `ENABLE_*` 双向比对，报「没有文件认领」和「文件声明了但配置里没有」。
- `ldd` 白名单里删掉了 `libz`（zlib 已私有化，所有消费者都有 RUNPATH 或在 `LD_LIBRARY_PATH` 里）。

## 五、工具（`60cc380`）

```bash
$ phprt modules
类别     名字       开关              依赖                                状态
builtin  gd         ENABLE_GD         libpng jpeg freetype webp aom avif  开
builtin  intl       ENABLE_INTL       icu                                 开
...
deps     libpng     -                 zlib                                已装
...
ext      swoole     ENABLE_SWOOLE     openssl,zlib                        已装
app      composer   ENABLE_COMPOSER   -                                   已装
```

```bash
$ phprt build --php 8.5.12 --show-args    # 只打印最终 configure 行，不编译
```

## 途中发现的问题

1. **`.gitignore` 把模块目录吞了**：`deps/`、`tools/` 没有锚定，`0f2c65b` 只提交了旧路径的删除，
   新的 `lib/modules/deps/*.sh`、`lib/modules/tools/*.sh`（19 个文件）没进 git。
   改成 `/deps/` `/tools/` `/build/` `/logs/` `/php-*/` `/bin/` 后在 `67e9a03` 补回。
2. **libavif 会拉系统的 libaom / libvmaf**：`ldd` 显示 `/lib64/libaom.so.3`、`/lib64/libvmaf.so.3`。
   原因是 libavif 没有 aom 的 RUNPATH，回落系统 aom，而系统 aom 链了 vmaf。
   加 `-DCMAKE_INSTALL_RPATH` 后指向 `deps/aom-3.12.0`，vmaf 一并消失。
3. **libz 白名单其实是死代码**：`check_ldd` 自己带了 `LD_LIBRARY_PATH=deps/*/lib`，
   去掉 `libz` 例外后两个版本都没有告警。真正需要它的是「开了 swoole 但没开 zlib」的配置。
4. **`builtin/*.sh` 的 source 路径多写了一级**：写成 `../../../common.sh` 后报
   `builtin/../../../common.sh: 没有那个文件或目录`，改成 `../../common.sh`。
5. **`.phprt-deps` 有重复行**：`libffi-3.4.8` 出现两次（`lib` 和 `lib64` 各一条），
   加上修复前 `enabled_dep_names` 会把 `openssl zlib` 当一行输出，末尾又多了两条。
   8.5.12 用修复后的代码 `--force --only php` 重建，现在是 16 条、无重复。
6. **`--only php` 现在会带出整个 needs 闭包**：因为 `module_needs php` 返回所有内置扩展的依赖。
   普通构建只是多跑几条「已安装，跳过」；带 `--force` 时等于全量重建（实测 8.5.12 走了约 5 分钟）。

## 实测

在两个版本上重编并对照改前的 `Configure Command`（用 `php -i` 抓，shlex 分词后只比 `--` 开头的项）：

| 项 | 8.5.12 | 8.6.0-dev |
| --- | --- | --- |
| configure 开关数 | 44 | 36 |
| 与改前逐字对比 | 一致 | 一致 |
| `phprt verify` | 全部自检通过 | 全部自检通过 |
| `5d` 开关归属 | 20 个开关 / 20 个文件 | 同左 |
| `.phprt-deps` | 16 条，无重复 | 2 条（sqlite、oniguruma） |
| 幂等构建 | 全部跳过 | 全部跳过 |

不带 `LD_LIBRARY_PATH` 的 `ldd`：

```
libpng16.so    libz.so.1   => deps/zlib-1.3.2/lib/libz.so.1
libfreetype.so libz.so.1   => deps/zlib-1.3.2/lib/libz.so.1
libavif.so     libaom.so.3 => deps/aom-3.12.0/lib/libaom.so.3
swoole.so      libz.so.1   => deps/zlib-1.3.2/lib/libz.so.1
xdebug.so      libz.so.1   => deps/zlib-1.3.2/lib/libz.so.1
```

`blog-rr`（RoadRunner + 8.6.0-dev）重启后 `/health` 正常，4 个 worker 的 `/proc/<pid>/maps` 都含 `protobuf.so`。

## 现在的目录

```
lib/modules/
├── deps/     16 个库模块
├── ext/      6 个 PECL 扩展
├── builtin/  11 个内置扩展声明（共 257 行）
├── tools/    3 个构建工具
├── php.sh    185 行（收集参数 + 公共扩展 + rpath）
└── composer.sh
```

## 没有做的

- 内置扩展仍不能编成独立 `.so`：zlib / openssl 的 `config0.m4` 不接受 `shared`，只能改 PHP 源码。
- `libxml2` / `libpng` / `freetype` 这类库自身对 zlib 的依赖仍靠 `LD_LIBRARY_PATH` 解析，
  没有逐个加 RUNPATH（`5c` 检查的是声明了 `needs=` 的模块，检查时按 `verify` 的规则带上了环境变量）。
- `--only php --force` 的行为没有拆开：目前等于「PHP + 全部依赖 + 全部扩展」。
