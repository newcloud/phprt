# 运行时库的 wheel 化：内容 hash + `$ORIGIN` rpath

phprt 的依赖库装在 `<tree>/deps/<库>-<版本>/lib`，如果直接把这一堆目录塞进
`LD_LIBRARY_PATH`，会遇到三个问题：

1. 同名库冲突（系统 `/lib64/libz.so.1` vs 树里的 `libz.so.1`）；
2. 运行时要拼环境变量，整棵树搬走或换 cwd 就失效；
3. 扩展 `.so` 依赖二级、三级库时，路径越拼越长。

做法照搬 Python 的 auditwheel：**把库拍平到一个目录，名字带内容 hash，
SONAME 一起改，所有消费方改 `DT_NEEDED`，RPATH 写成 `$ORIGIN` 相对路径。**

## 1. 结果长什么样

```text
<tree>/
├── bin/php                 RPATH $ORIGIN/../lib/phprt
├── lib/phprt/              ← 所有依赖库，平铺
│   ├── libz-523f183f.so.1
│   ├── libxml2-048dd98f.so.16
│   ├── libssl-7ce83f8a.so.3
│   ├── libcrypto-2c33ab84.so.3
│   ├── ...
│   └── wheel.map           旧 SONAME → 新名字
├── lib/php/extensions/*/   RPATH $ORIGIN/../../../phprt
└── deps/<库>-<版本>/lib/   构建期链接用，运行时不再需要
```

`wheel.map` 就是一行一条映射：

```text
libxml2.so.16 libxml2-048dd98f.so.16
libssl.so.3   libssl-7ce83f8a.so.3
libz.so.1     libz-523f183f.so.1
```

## 2. 五步算法

实现在 `runner/common.sh` 的 `wheelize_tree()`，构建收尾时调用一次。

### 第一步：用 SONAME 算名字，不是文件名

```bash
soname="$(patchelf --print-soname "$f" 2>/dev/null || true)"
[ -n "$soname" ] || soname="$(basename "$f")"
```

`libzip.so.5.6` 的真实文件，SONAME 是 `libzip.so.5`；消费方的 `DT_NEEDED` 记的也是
`libzip.so.5`。按文件名算出来的名字没人引用，链接会找一个不存在的库。

### 第二步：hash 在改文件**之前**算

```bash
hash="$(sha256sum "$f" | cut -c1-8)"
```

取名规则：

```bash
wheel_hashed_name() {   # $1=SONAME  $2=hash
  case "$1" in
    *.so)   printf '%s-%s.so\n'   "${1%.so}"    "$2" ;;   # libfoo.so   → libfoo-<h>.so
    *.so.*) printf '%s-%s.so%s\n' "${1%%.so.*}" "$2" "${1#*.so}" ;;  # libfoo.so.1 → libfoo-<h>.so.1
    *)      printf '%s-%s\n'      "$1"          "$2" ;;
  esac
}
```

hash 取完整内容 hash 的前 8 位，而不是「版本 + 构建时间」：内容变了名字一定变，
内容一样就一定一样，便于判断两个树里的库是不是同一份。

### 第三步：复制、改 SONAME、设 `$ORIGIN`

```bash
cp -af "$f" "$flat/$new"
patchelf --set-soname "$new" --set-rpath '$ORIGIN' --force-rpath "$flat/$new"
```

- `--set-soname`：库自己的 SONAME 必须跟新名字一致，否则加载器按 SONAME 找不到；
- `--set-rpath '$ORIGIN'`：库里找同级依赖（`libssl` → `libcrypto`）只看自己所在目录；
- `--force-rpath`：**必须加**，见下面第 4 节。

### 第四步：回写所有消费方的 `DT_NEEDED`

```bash
while read -r old new; do
  for elf in "$tree/bin/php" "$flat"/*.so* "$tree"/lib/*.so* \
             "$tree"/lib/php/extensions/*/*.so; do
    [ -f "$elf" ] || continue
    [ -L "$elf" ] && continue
    patchelf --replace-needed "$old" "$new" "$elf" 2>/dev/null || true
  done
done < "$flat/wheel.map"
```

`--set-soname` 只改库自己，已经编译好的 `bin/php`、`curl.so`、`swoole.so` 里写的还是
旧名字，必须一起改。

### 第五步：给每个 ELF 设相对 RPATH

```bash
patchelf --set-rpath '$ORIGIN/../lib/phprt'    --force-rpath "$tree/bin/php"
patchelf --set-rpath '$ORIGIN'                 --force-rpath "$flat"/*.so*
patchelf --set-rpath '$ORIGIN/../../../phprt'  --force-rpath "$tree"/lib/php/extensions/*/*.so
```

路径是按 ELF 自身位置算的，所以整棵树搬走、改名都不影响：

```text
<tree>/bin/php                                → ../lib/phprt
<tree>/lib/phprt/libssl-<h>.so.3              → 同目录
<tree>/lib/php/extensions/<abi>/yaml.so       → ../../../phprt
```

## 3. 为什么不能只靠 `LD_LIBRARY_PATH`

`LD_LIBRARY_PATH` 是进程级、按目录搜索：

- 进程里同时有系统 `libz.so.1` 和树里的 `libz.so.1` 时，谁先被加载取决于搜索顺序；
- `curl.so` 通过 `dlopen` 加载进来的子库，会继承祖先的 `RPATH` 链，但不会继承
  「加载时的目录」；
- 拼路径的脚本一多，`myphp`、worker、cron 各写一份，迟早不一致。

hash 改名把第一条彻底解决：树里的 `libz-523f183f.so.1` 和系统的 `libz.so.1`
是两个不同的 SONAME。`$ORIGIN` 解决后两条。

## 4. 为什么必须 `--force-rpath`

`patchelf --set-rpath` 默认写 `DT_RUNPATH`（`0x1d`），加 `--force-rpath` 才写
`DT_RPATH`（`0xf`）。两者的区别在传递性：

- `DT_RPATH`：会沿依赖链向下传递，`bin/php` 的 rpath 对 `bin/php` 加载的子库也生效；
- `DT_RUNPATH`：只对直接依赖生效，孙辈库找不到。

实测里 Python 生态的 wheel（numpy / pillow / shapely）也全是 `DT_RPATH`。
phprt 这里同样用 `DT_RPATH`，保证三级链（`php → libssl → libcrypto`）不需要
每层都重复同一串绝对路径。

## 5. 增量重建：hash 会变，怎么办

依赖重编后内容变了，hash 变、名字变，树里已经改写过的 `DT_NEEDED` 会指向被删掉的旧文件。
`wheelize_tree` 用上一轮的映射兜住：

```bash
# 1) 先备份上一轮的 wheel.map
[ -f "$flat/wheel.map" ] && cp -f "$flat/wheel.map" "$prev"

# 2) 生成替换表：旧 SONAME → 新名字，以及上一轮的 hash 名 → 本轮新名字
cp -f "$flat/wheel.map" "$repl"
while read -r osoname onew; do
  new="$(awk -v s="$osoname" '$1 == s { print $2 }' "$flat/wheel.map")"
  [ -n "$new" ] && [ "$onew" != "$new" ] && printf '%s %s\n' "$onew" "$new" >> "$repl"
done < "$prev"

# 3) 按替换表批量 --replace-needed
# 4) 删除旧文件（rm -f "$flat"/*.so* 在开头）
```

所以三种情况都安全：

| 场景 | 结果 |
| --- | --- |
| 同一棵树重复跑 | 名字不变（幂等，实测连跑 3 次 md5 一致） |
| 依赖重编、hash 变 | 旧引用自动改到新名字，旧文件清理掉 |
| 只搬目录、不重建 | 名字不变（hash 是内容 hash，与路径无关） |

## 6. 踩过的坑（都是实测）

### 6.1 真实文件名等于 SONAME 时会被软链覆盖

早期实现会在 `deps/*/lib` 里建一个 dev 软链指向 `lib/phprt`，用于让后续 `add ext`
直接链接到 hash 名字。当某个库的真实文件名恰好等于 SONAME 时，`ln -sfn` 会把源文件
替换成软链，第二轮就找不到原始文件。

结论：**不要动 `deps/*/lib`**，保持原样（见 6.2）。

### 6.2 把 `deps` 的软链指向 `lib/phprt` 会让后续链接失败

如果让 `deps/libxml2-2.15.4/lib/libxml2.so` 指向 `lib/phprt/libxml2-<h>.so.16`，
下一次 `add ext` 链接时 `ld` 需要解析二级依赖：

```text
ld: warning: libz-523f183f.so.1, needed by .../libxml2.so, not found
ld: undefined reference to `gzwrite'
```

因为 `libz-523f183f.so.1` 在 `lib/phprt`，而构建期的 `-L` 只指向 `deps/*/lib`。
表现是 PHP 重新 configure 时报 `iconv check failed`，很难联想到 rpath。

正确做法：`deps/*/lib` 保持原始名字，只在 `lib/phprt` 放改过名的副本，
消费方的名字由 `--replace-needed` 统一改写。

### 6.3 hash 会随构建目录变（libxml2）

同一个 libxml2 在三个目录构建出三个 hash（`1d407334` / `930ecf14` / `4b343735`），
而 zlib 在三个目录都是 `523f183f`。原因不是「重新编译」，而是 libxml2 的二进制里
编译进了绝对路径：

```text
file:///tmp/rpath2/.phprt/php-8.5.12/deps/libxml2-2.15.4/etc/xml/catalog
/tmp/rpath2/.phprt/build/libxml2-2.15.4
/tmp/rpath2/.phprt/php-8.5.12/deps/zlib-1.3.2/include
```

zlib 里一条绝对路径都没有，所以稳定。

结论：hash 只保证「同一份字节同一个名字」，不保证跨目录构建一致。要跨目录稳定，
得在 recipe 里做可复现构建（`-ffile-prefix-map`、固定 `--prefix` 等），当前没做。

### 6.4 按名字 `dlopen` 的库找不到

`DT_NEEDED` 会被自动改写，但代码里写死的 `dlopen("libzip.so.5")` 不会。
`lib/phprt` 里只有带 hash 的名字，这种调用会失败。

目前 17 个扩展没遇到。真遇到就在 `lib/phprt` 里补一个不带 hash 的别名软链。

### 6.5 libtool 的 `.la` 会带绝对 rpath

有些库用 libtool，链接时会把 `-L` 目录写进 `DT_RUNPATH`。`--set-rpath` 最后会覆盖
`bin/php` 和扩展，但 `deps` 里的中间产物不管（运行时不使用）。如果某个产物在覆盖范围外，
用 `--disable-rpath` 或在链接前临时移走 `.la`。

## 7. 怎么验证

```bash
T=.phprt/php-8.5.12

# 1) 映射
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
```

`phprt verify` 的第 3、4 步就是自动化版本的 3 和 4。

## 8. 实测数据（PHP 8.5.12，17 个扩展）

19 个库被拍平：

```text
libcurl libfreetype libicudata libicui18n libicuio libicutest libicutu libicuuc
libjpeg libpng16 libxml2 libyaml-0 libzip libonig libcrypto libssl libsqlite3
libz libffi
```

`bin/php` 的 `DT_NEEDED`：

```text
libssl-7ce83f8a.so.3      libcrypto-2c33ab84.so.3   libcurl-ca1f9245.so.4
libz-523f183f.so.1        libffi-629edaa4.so.8      libjpeg-0ba68363.so.62
libfreetype-2537d6ab.so.6 libpng16-1b4067f2.so.16   libicuuc-e44d2add.so.79
libicuio-f7c6bfd2.so.79   libicui18n-9b8c2e62.so.79 libonig-9c72400d.so.5
libxml2-048dd98f.so.16    libzip-8a3b4465.so.5
```

二级、三级链：

```text
libssl   → libcrypto-2c33ab84.so.3      RPATH $ORIGIN
libcurl  → libssl / libcrypto / libz    RPATH $ORIGIN
libicuuc → libicudata-e08b4de4.so.79    RPATH $ORIGIN
yaml.so  → libyaml-0-991d32c0.so.2      RPATH $ORIGIN/../../../phprt
```

验证结果：

- `ldd` 里树外的非 glibc 库：0 个
- `myphp` 里 `LD_LIBRARY_PATH`：0 处
- `env -i ./myphp -r '...'`：17 个扩展功能全部正常
- 整棵树搬到新目录后：PHP、swoole、zip、yaml 全部正常
- `phprt verify`：4 步全过

## 9. 局限

- **只处理共享库。** 静态库（`.a`）不参与，也不需要。
- **只处理 `deps/*/lib{,64}/*.so.*` 的真实文件。** 软链跳过，其他目录不扫描。
- **依赖 `patchelf`。** 系统没有时构建会直接报错（`need_cmds patchelf`）。
- **不能替代可复现构建。** hash 反映内容，内容包含编译期路径；换目录重编名字会变。
- **`dlopen` 字符串不会改。** 见 6.4。

如果以后要在别的平台（macOS）上用，思路一样，工具换成 `install_name_tool`
+ `@loader_path`，但 hash 命名和 `wheel.map` 的格式可以保持一致。
