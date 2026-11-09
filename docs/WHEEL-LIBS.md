# 运行时库的 wheel 化：运行时 SONAME（rt）+ `$ORIGIN` rpath

recipe 侧只需要遵守「按标准方式安装」，例外用 `wheel=` 声明；约定见
[RECIPE.md 第 6 节](RECIPE.md#6-wheel-化与-soname)。本文讲 driver 侧的算法和踩过的坑。

phprt 的依赖库装在 `<tree>/deps/<库>-<版本>/lib`，如果直接把这一堆目录塞进
`LD_LIBRARY_PATH`，会遇到三个问题：

1. 同名库冲突（系统 `/lib64/libz.so.1` vs 树里的 `libz.so.1`）；
2. 运行时要拼环境变量，整棵树搬走或换 cwd 就失效；
3. 扩展 `.so` 依赖二级、三级库时，路径越拼越长。

做法照搬 Python 的 auditwheel：**把库拍平到一个目录，改成稳定的运行时名
（rt + SONAME 的 ABI 版本号），SONAME 同步改写，所有消费方改 `DT_NEEDED`，
RPATH 写成 `$ORIGIN` 相对路径。**

命名不用内容 hash，而是用上游 SONAME 自带的 ABI 版本号：

```text
libssl.so.3    → libssl-rt3.so.3
libcrypto.so.3 → libcrypto-rt3.so.3
libz.so.1      → libz-rt1.so.1
libicuuc.so.79 → libicuuc-rt79.so.79
libpng16.so.16 → libpng16-rt16.so.16
libyaml-0.so.2 → libyaml-0-rt2.so.2
```

SONAME 就是上游的 ABI 系列信号：同系列小版本升级（如 OpenSSL
3.5.6 → 3.5.8）SONAME 不变，运行时名就不变，消费方的 `DT_NEEDED` 零改写、
也不需要重编；跨 ABI 系列（SONAME 变化）才改名并改写消费方。内容完整性
由 `wheel.map` 里的 sha256 列负责，不再体现在文件名里。

## 1. 结果长什么样

```text
<tree>/
├── bin/php                 RPATH $ORIGIN/../lib/phprt
├── lib/phprt/              ← 所有依赖库，平铺
│   ├── libz-rt1.so.1
│   ├── libxml2-rt16.so.16
│   ├── libssl-rt3.so.3
│   ├── libcrypto-rt3.so.3
│   ├── ...
│   └── wheel.map           原始SONAME|运行时名|版本|sha256
├── lib/php/extensions/*/   RPATH $ORIGIN/../../../phprt
└── deps/<库>-<版本>/lib/   构建期链接用，运行时不再需要
```

`wheel.map` 一行一条，pipe 分隔四列：

```text
libxml2.so.16|libxml2-rt16.so.16|2.15.4|<sha256>
libssl.so.3|libssl-rt3.so.3|3.5.6|<sha256>
libz.so.1|libz-rt1.so.1|1.3.2|<sha256>
```

## 2. 六步算法

实现在 `runner/common.sh` 的 `wheelize_tree()`，构建收尾时调用一次。

### 第一步：用 SONAME 算名字，不是文件名

```bash
soname="$(patchelf --print-soname "$f" 2>/dev/null || true)"
[ -n "$soname" ] || soname="$(basename "$f")"
```

`libzip.so.5.6` 的真实文件，SONAME 是 `libzip.so.5`；消费方的 `DT_NEEDED`
记的也是 `libzip.so.5`。按文件名算出来的名字没人引用，链接会找一个不存在
的库。

### 第二步：运行时名 = SONAME 主体 + rt + ABI 版本

```bash
wheel_runtime_name() {   # $1=SONAME
  case "$1" in
    *.so.*) printf '%s-rt%s.so.%s\n' "${1%%.so.*}" "${1##*.so.}" "${1##*.so.}" ;;
    *.so)   printf '%s-rt.so\n'   "${1%.so}" ;;
    *)      printf '%s-rt\n'      "$1" ;;
  esac
}
```

rt 标签取 SONAME 的版本号（`.so.N` 的 N），不取包版本：包版本里才有的
minor/patch 差异不该体现在链接名上。

### 第三步：复制、改 SONAME、设 `$ORIGIN`

```bash
cp -af "$f" "$flat/$new"
patchelf --set-soname "$new" --set-rpath '$ORIGIN' --force-rpath "$flat/$new"
```

- `--set-soname`：库自己的 SONAME 必须跟新名字一致，否则加载器按 SONAME
  找不到；
- `--set-rpath '$ORIGIN'`：库里找同级依赖（`libssl` → `libcrypto`）只看自己
  所在目录；
- `--force-rpath`：**必须加**，见下面第 4 节。

### 第四步：回写所有消费方的 `DT_NEEDED`

```bash
while IFS='|' read -r old new rest; do
  for elf in "$tree/bin/php" "$flat"/*.so* "$tree"/lib/*.so* \
             "$tree"/lib/php/extensions/*/*.so; do
    [ -f "$elf" ] || continue
    [ -L "$elf" ] && continue
    readelf -h "$elf" >/dev/null 2>&1 || continue
    patchelf --replace-needed "$old" "$new" "$elf" 2>/dev/null || true
  done
done < "$flat/.replace"
```

`--set-soname` 只改库自己，已经编译好的 `bin/php`、`curl.so`、`swoole.so`
里写的还是旧名字，必须一起改。

### 第五步：给每个 ELF 设相对 RPATH

```bash
patchelf --set-rpath '$ORIGIN/../lib/phprt'    --force-rpath "$tree/bin/php"
patchelf --set-rpath '$ORIGIN'                 --force-rpath "$flat"/*.so*
patchelf --set-rpath '$ORIGIN/../../../phprt'  --force-rpath "$tree"/lib/php/extensions/*/*.so
```

路径是按 ELF 自身位置算的，所以整棵树搬走、改名都不影响。

### 第六步：回填 sha256

`set-soname`、`--replace-needed`、`--set-rpath` 都会改字节，所以 map 里的
sha256 列在所有 patchelf 之后才回填：

```bash
while IFS='|' read -r src rt ver sha0; do
  printf '%s|%s|%s|%s\n' "$src" "$rt" "$ver" \
    "$(sha256sum "$flat/$rt" | cut -d' ' -f1)" >> "$flat/.wheel.map.tmp"
done < "$flat/wheel.map"
mv -f "$flat/.wheel.map.tmp" "$flat/wheel.map"
```

## 3. 为什么不能只靠 `LD_LIBRARY_PATH`

`LD_LIBRARY_PATH` 是进程级、按目录搜索：

- 进程里同时有系统 `libz.so.1` 和树里的 `libz.so.1` 时，谁先被加载取决于
  搜索顺序；
- `curl.so` 通过 `dlopen` 加载进来的子库，会继承祖先的 `RPATH` 链，但不会
  继承「加载时的目录」；
- 拼路径的脚本一多，`myphp`、worker、cron 各写一份，迟早不一致。

rt 改名把第一条彻底解决：树里的 `libz-rt1.so.1` 和系统的 `libz.so.1` 是
两个不同的 SONAME。`$ORIGIN` 解决后两条。

## 4. 为什么必须 `--force-rpath`

`patchelf --set-rpath` 默认写 `DT_RUNPATH`（`0x1d`），加 `--force-rpath` 才写
`DT_RPATH`（`0xf`）。两者的区别在传递性：

- `DT_RPATH`：会沿依赖链向下传递，`bin/php` 的 rpath 对 `bin/php` 加载的
  子库也生效；
- `DT_RUNPATH`：只对直接依赖生效，孙辈库找不到。

实测里 Python 生态的 wheel（numpy / pillow / shapely）也全是 `DT_RPATH`。
phprt 这里同样用 `DT_RPATH`，保证三级链（`php → libssl → libcrypto`）不需要
每层都重复同一串绝对路径。

## 5. 增量重建：跨系列才改名

依赖重编后运行时名可能变（跨 ABI 系列），树里已经改写过的 `DT_NEEDED` 会
指向被删掉的旧文件。`wheelize_tree` 用上一轮的映射兜住：

```bash
# 1) 先备份上一轮映射
[ -f "$flat/wheel.map" ] && cp -f "$flat/wheel.map" "$prev"

# 2) 生成替换表：本轮 原始SONAME→运行时名，以及上一轮运行时名→本轮运行时名
#    （名字没变就不追加，零改写）

# 3) 按替换表批量 --replace-needed
# 4) 删除旧文件（rm -f "$flat"/*.so* 在开头）
```

三种情况：

| 场景 | 结果 |
| --- | --- |
| 同一棵树重复跑 | 名字不变（幂等，实测连跑结果一致） |
| 同系列小版本升级（`phprt update`） | 运行时名不变，消费方零改写 |
| 跨 ABI 系列（SONAME 变化） | 名字变，旧引用自动改到新名字 |

hash 时代迁移到 rt 命名时，`.prev` 里的旧 hash 名会被一次性改写到 rt 名，
只需要一次。

## 6. 踩过的坑（都是实测）

### 6.1 真实文件名等于 SONAME 时会被软链覆盖

早期实现会在 `deps/*/lib` 里建一个 dev 软链指向 `lib/phprt`，用于让后续
`add ext` 直接链接到运行时名。当某个库的真实文件名恰好等于 SONAME 时，
`ln -sfn` 会把源文件替换成软链，第二轮就找不到原始文件。

结论：**不要动 `deps/*/lib`**，保持原样（见 6.2）。

### 6.2 把 `deps` 的软链指向 `lib/phprt` 会让后续链接失败

如果让 `deps/libxml2-2.15.4/lib/libxml2.so` 指向 `lib/phprt/libxml2-rt16.so.16`，
下一次 `add ext` 链接时 `ld` 需要解析二级依赖：

```text
ld: warning: libz-rt1.so.1, needed by .../libxml2.so, not found
ld: undefined reference to `gzwrite'
```

因为 `libz-rt1.so.1` 在 `lib/phprt`，而构建期的 `-L` 只指向 `deps/*/lib`。
表现是 PHP 重新 configure 时报 `iconv check failed`，很难联想到 rpath。

正确做法：`deps/*/lib` 保持原始名字，只在 `lib/phprt` 放改过名的副本，
消费方的名字由 `--replace-needed` 统一改写。

### 6.3 运行时名与构建目录无关（rt 方案比 hash 强的地方）

hash 方案里同一个 libxml2 在三个目录构建出三个名字（二进制里编译进了
绝对路径），zlib 则稳定。rt 命名只看 SONAME，换目录、换编译选项只要 SONAME
不变，名字就不变——消费方的 `DT_NEEDED` 稳定。代价是「同名不一定同内容」，
由 wheel.map 的 sha256 列和 `phprt verify` 补回来（见 6.6）。

### 6.4 按名字 `dlopen` 的库找不到

`DT_NEEDED` 会被自动改写，但代码里写死的 `dlopen("libzip.so.5")` 不会。
`lib/phprt` 里只有 rt 名，这种调用会失败。

目前 17 个扩展没遇到。真遇到就在 `lib/phprt` 里补一个原始 SONAME 的别名
软链（指向 rt 名）。

### 6.5 libtool 的 `.la` 会带绝对 rpath

有些库用 libtool，链接时会把 `-L` 目录写进 `DT_RUNPATH`。`--set-rpath` 最后
会覆盖 `bin/php` 和扩展，但 `deps` 里的中间产物不管（运行时不使用）。如果
某个产物在覆盖范围外，用 `--disable-rpath` 或在链接前临时移走 `.la`。

### 6.6 同名不代表同 ABI

rt 名只看 SONAME，同一系列内换编译选项（如 OpenSSL 的 `enable-deprecated`）
可能改变 ABI 而名字不变。防线上有三道：

- `phprt update` 升级前后比对 SONAME 集合，不一致当跨系列拒绝；
- `wheel.map` 记 sha256，`phprt verify` 逐份校验；
- 升级日志 `.phprt/logs/upgrade.log` 留下版本与 sha256，可追溯。

## 7. 怎么验证

```bash
T=.phprt/php-8.5.12

# 1) 映射（4 列 pipe 分隔）
cat $T/lib/phprt/wheel.map

# 2) 二进制里的名字和 rpath
readelf -d $T/bin/php | grep -E 'NEEDED|RPATH|RUNPATH'

# 3) 依赖是否全部落在树内（除 glibc 等基础库）
ldd $T/bin/php | grep -vE '/(libc|libm|libgcc_s|libstdc\+\+|libpthread|libdl|librt|ld-linux)'

# 4) 不得出现绝对路径 RPATH
find $T -name '*.so*' -o -name php | while read f; do
  readelf -d "$f" 2>/dev/null | grep -E 'RPATH|RUNPATH' | grep -v '\$ORIGIN' && echo "绝对路径: $f"
done

# 5) 不靠环境变量能跑
env -i ./myphp -v

# 6) 整棵树搬走后还能跑
mv /path/to/project /tmp/moved && cd /tmp/moved && env -i ./myphp -v

# 7) phprt verify 的第 7 步：wheel 完整性
#    （sha256、DT_SONAME 与文件名一致、OpenSSL 唯一且成套、无残留原始 SONAME）
phprt verify
```

`phprt verify` 的第 3、4、7 步就是上面 3、4 和完整性检查的自动化版本。

## 8. 实测数据（PHP 8.5.12，17 个扩展）

19 个库被拍平：

```text
libcurl libfreetype libicudata libicui18n libicuio libicutest libicutu libicuuc
libjpeg libpng16 libxml2 libyaml-0 libzip libonig libcrypto libssl libsqlite3
libz libffi
```

`bin/php` 的 `DT_NEEDED`（rt 名）：

```text
libssl-rt3.so.3      libcrypto-rt3.so.3     libcurl-rt4.so.4
libz-rt1.so.1        libffi-rt8.so.8        libjpeg-rt62.so.62
libfreetype-rt6.so.6 libpng16-rt16.so.16    libicuuc-rt79.so.79
libicuio-rt79.so.79  libicui18n-rt79.so.79  libonig-rt5.so.5
libxml2-rt16.so.16   libzip-rt5.so.5
```

二级、三级链：

```text
libssl   → libcrypto-rt3.so.3        RPATH $ORIGIN
libcurl  → libssl / libcrypto / libz RPATH $ORIGIN
libicuuc → libicudata-rt79.so.79     RPATH $ORIGIN
yaml.so  → libyaml-0-rt2.so.2        RPATH $ORIGIN/../../../phprt
```

验证结果：

- `ldd` 里树外的非 glibc 库：0 个
- `myphp` 里 `LD_LIBRARY_PATH`：0 处
- `env -i ./myphp -r '...'`：17 个扩展功能全部正常
- 整棵树搬到新目录后：PHP、swoole、zip、yaml 全部正常
- `phprt verify`：7 步全过

## 9. 局限

- **只处理共享库。** 静态库（`.a`）不参与，也不需要。
- **只处理 `deps/*/lib{,64}/*.so.*` 的真实文件。** 软链跳过，其他目录不扫描。
- **依赖 `patchelf`。** 系统没有时构建会直接报错（`need_cmds patchelf`）。
- **rt 名只表达 ABI 系列，不表达内容。** 同系列内 rebuild 名字不变，内容
  差异靠 wheel.map 的 sha256 列和 verify 校验；要跨目录比对内容，看
  wheel.map 的 sha256，不用比文件名。
- **`dlopen` 字符串不会改。** 见 6.4。

同系列小版本升级走 `phprt update <依赖> <版本>`（见
`docs/RUNTIME-LIB-VERSIONING.md` 第 9 节）：只重编该依赖、替换运行时库，
消费方零改写、零重编。
