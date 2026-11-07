# 给 bundle 加扩展或依赖

bundle 是仓库里 `bundles/php<系列>/` 那一套文件：版本信息、configure 逻辑和 recipe。
加东西只改仓库，`install` / `add ext` 会把改过的 bundle 和 runner 同步到项目。

## 1. 三类节点

| 类型 | 位置 | 作用 | 产物 |
| --- | --- | --- | --- |
| 内置扩展 | `builtin/<扩展>.sh` | 由 PHP 本体 configure 编译 | 编进 `bin/php`，或 `lib/php/extensions/*/` 下的 `.so` |
| 依赖库 | `deps/<库>.sh` | 第三方 C/C++ 库 | `<tree>/deps/<库>-<版本>/` |
| PECL 扩展 | `ext/<扩展>.sh` | phpize + configure + make | `<tree>/lib/php/extensions/*/<扩展>.so` |

runtime 库不用管：`set` 收尾的 `wheelize_tree` 会自动把 `deps/*/lib` 里的 `.so`
改成 rt 运行时名、改 SONAME、回写消费方的 `DT_NEEDED`，并把 RPATH 设成 `$ORIGIN`。

## 2. 加一个依赖库

以 `libfoo` 为例，要改四处：

### 2.1 `bundle.conf`：版本和下载地址

```conf
LIBFOO_VERSION=1.2.3
LIBFOO_URL="https://example.com/libfoo-1.2.3.tar.xz"
LIBFOO_SHA256=""
```

`SHA256` 留空时首次下载后记到 `downloads/SHA256SUMS`（TOFU），填了就强制校验。
文件名取 URL 最后一段，源码缓存里必须叫这个名字。

### 2.2 `runner/common.sh`：目录映射

`dep_dir_of` 决定安装目录，不加会直接 `die "未知依赖"`：

```bash
libfoo)    dep_dir libfoo    "${LIBFOO_VERSION:-}" ;;
```

### 2.3 `bundles/php<系列>/deps/libfoo.sh`

```bash
#!/usr/bin/env bash
# =============================================================================
#  libfoo
#  phprt: needs=zlib          # 传递依赖，可选；多个用逗号分隔
#  安装到: deps/libfoo-<版本>/
# =============================================================================
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

need_cmds cmake make tar

D="$(dep_dir_of libfoo)"
SIG="libfoo=$LIBFOO_VERSION;zlib=${ENABLE_ZLIB:-0}"

if [ "$FORCE" != 1 ] && stamp_ok libfoo "$SIG" && [ -f "$D/lib/libfoo.so" ]; then
  ok "libfoo 已安装，跳过"; exit 0
fi

ensure_clean_target libfoo "$D"

TAR="$(basename "$LIBFOO_URL")"
fetch "$LIBFOO_URL" "$DOWNLOADS/$TAR" "${LIBFOO_SHA256:-}"

SRC="$SRCDIR/libfoo-$LIBFOO_VERSION"
if [ "$REBUILD" = 1 ] && [ -d "$SRC" ]; then
  log "复用源码 $SRC"
else
  rm -rf "$SRC"; mkdir -p "$SRC"
  log "解压 $TAR"
  extract "$DOWNLOADS/$TAR" "$SRC"
fi

cd "$SRC"
log "configure + make -j$JOBS"
./configure --prefix="$D" \
    --enable-shared --disable-static \
    CFLAGS="$(dep_include_flags $(enabled_dep_names))" \
    LDFLAGS="$(dep_build_flags $(enabled_dep_names))" \
    PKG_CONFIG_PATH="$(dep_pc_path $(enabled_dep_names))" \
    > "$LOGS/libfoo-configure.log" 2>&1 \
  || { tail -30 "$LOGS/libfoo-configure.log"; die "libfoo configure 失败"; }

make -j"$JOBS"             > "$LOGS/libfoo-make.log" 2>&1 || die "libfoo 编译失败"
make install               >> "$LOGS/libfoo-make.log" 2>&1 || die "libfoo 安装失败"

stamp_write libfoo "$SIG"
ok "libfoo $LIBFOO_VERSION -> $D"
```

约定：

- 必须有 `.so` 或 `.a` 落到 `$D`，否则 `artifact_ok` 判定失败，构建直接报错；
- 用 `dep_dir_of`、`dep_build_flags`、`dep_include_flags`、`dep_pc_path`，别手写路径；
- 签名 `SIG` 里要带上影响产物的开关和版本，否则改配置不会重编。

### 2.4 让 PHP 或扩展用上它

内置扩展里：

```bash
--flags)  printf '%s\n' "--with-foo=$(dep_dir_of libfoo)" ;;
--needs)  printf '%s\n' libfoo ;;
```

构建时 `LD_LIBRARY_PATH` 会自动包含所有已选依赖的 `lib`（`dep_env`），
不用手写 rpath。

## 3. 加一个内置扩展

`builtin/<扩展>.sh`，由 `php.sh` 调用，不单独执行：

```bash
#!/usr/bin/env bash
# 内置扩展：foo
# phprt: builtin=foo switch=ENABLE_FOO
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

case "${1:-}" in
  --flags)
    if [ "${ENABLE_FOO:-0}" = 1 ]; then
      printf '%s\n' "--enable-foo"
    else
      printf '%s\n' "--disable-foo"
    fi ;;
  --needs)
    if [ "${ENABLE_FOO:-0}" = 1 ]; then printf '%s\n' libfoo; fi ;;
  *) die "由 php.sh 调用: $0 --flags | --needs" ;;
esac
```

还要改三处：

1. `runner/build.sh` 的 `BUILTIN_ORDER` 里加 `foo`（决定 configure 顺序，依赖要在前面）；
2. `runner/common.sh` 的 `ext_modules` 里加 `foo) printf '%s\n' foo ;;`（`verify` 和 `enable_ext` 用）；
3. bundle.conf 的 `SUPPORTED_EXTS` / `DEFAULT_EXTS`。

`switch=` 那行的变量名会被 runner 按选择自动设成 1/0，不需要在 `bundle.conf` 里写。

## 4. 加一个 PECL 扩展

`ext/<扩展>.sh`，独立执行，参考 `ext/redis.sh`：

- `ensure_needs <扩展>` 先装依赖；
- `dep_env <扩展>` 设置构建期环境；
- `phpize` → `./configure --with-php-config="$PREFIX/bin/php-config"` → `make` → `make install`；
- 产物必须在 `$("$PREFIX/bin/php-config" --extension-dir)/<模块>.so`；
- 写 `$PREFIX/etc/conf.d/<序号>-<扩展>.ini` 才会启用（`$PREFIX` 就是版本树，
  即 `.phprt/php-<版本>`；这是 PHP 官方的 scan-dir 位置）；
- `stamp_write` 记录签名。

另外要改：

1. `runner/common.sh` 的 `ext_version`（版本号来自哪个变量）和 `ext_modules`（模块名）；
2. bundle.conf 的 `SUPPORTED_EXTS`；
3. 依赖写在脚本头部注释：`#  phprt: switch=ENABLE_FOO needs=libyaml`。

## 5. 自检清单

新增一个依赖 + 扩展，按顺序确认：

- [ ] `bundle.conf` 有 `*_VERSION` / `*_URL` / `*_SHA256`
- [ ] `runner/common.sh` 的 `dep_dir_of` 有分支
- [ ] `deps/<库>.sh` 能单独跑通：`bash bundles/php8.5/deps/libfoo.sh`
- [ ] 依赖装完 `deps/<库>-<版本>/lib` 里有 `.so`
- [ ] 内置扩展加了 `BUILTIN_ORDER` / `ext_modules`
- [ ] PECL 扩展加了 `ext_version` / `ext_modules`
- [ ] `SUPPORTED_EXTS` 里有名字
- [ ] `phprt install 8.5 --ext "<新扩展>" --plan` 能看到正确的依赖顺序
- [ ] 构建后 `readelf -d` 里新库的 `NEEDED` 是 rt 运行时名，`RPATH` 是 `$ORIGIN`
- [ ] `env -i ./myphp -m` 能看到扩展，`phprt verify` 全部步骤通过

## 6. 常见坑

**libtool 会把依赖目录写成绝对 RPATH。** 有些库的 `.la` 里记着绝对路径，链接时被塞进
`DT_RUNPATH`。构建完用 `readelf -d` 检查，出现绝对路径就要在 recipe 里
`--disable-rpath` 或链接前临时移走 `.la`。

**按文件名而不是 SONAME 改名会错位。** `libzip.so.5.6` 的 SONAME 是 `libzip.so.5`，
消费方记的是后者。`wheelize_tree` 已经按 SONAME 处理，手写脚本时别搞反。

**`deps/*/lib` 只给构建期用。** 不要把它里面的 `.so` 软链指向 `lib/phprt`：
后续 `add ext` 链接时 `ld` 跟不上二级依赖的 rt 运行时名，会报 `undefined reference`。

**运行时 `dlopen("libfoo.so.1")` 会失败。** `lib/phprt` 里只有 rt 运行时名，
`DT_NEEDED` 会被自动改写，字符串形式的 `dlopen` 不会。真遇到就补一个不带 rt 的别名。

**静态库不要参与 wheel 化。** `wheelize_tree` 只处理 `deps/*/lib/*.so.*` 的真实文件。
