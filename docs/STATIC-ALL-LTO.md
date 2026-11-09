# `--static-all-lto`：PHP 和依赖都开 LTO

本文定义 `phprt install --static-all-lto`。它是 `--static-all` 的加强版：
在单个 `bin/php` 的基础上，PHP 本体和第三方依赖都用 LTO 编译。

## 总结

- **状态**：v0 已实现；默认仍是 `--static-all`，`--static-all-lto` 是可选实验项。
- **用法**：

  ```bash
  phprt install 8.5 --static-all-lto \
    --ext "openssl zlib libxml2 curl sqlite mbstring"
  ```

- **当前覆盖**：
  - PHP 本体 + 依赖：`openssl zlib curl sqlite oniguruma libxml2`；
  - `--static-all` 的扩展和依赖全部编入单个 `bin/php`，glibc 仍动态；
  - `libzip`、`libsodium` 还未接，所以 `zip`、`sodium` 暂时不能进 LTO 集合；
  - `type=shared` 的扩展（xdebug/swoole）自动排除。
- **做法**：
  - 保留各 recipe 的 `-O2`/`-O3` 基线，只追加 `-flto=auto`；
  - 依赖 `.a` 用 `gcc-ar`/`gcc-ranlib` 生成，构建后检查 `.gnu.lto` 段；
  - PHP 静态链接沿用 `static-libs` 目录 + 重复 `EXTRA_LIBS`；
  - 最终 `bin/php` 检查 `NEEDED` 只允许 glibc，且不残留 `.gnu.lto` 段。
- **实测结论**：
  - 官方 `Zend/bench.php` 三组（bare/opcache/JIT）都是 0%；
  - `micro_bench.php` -1.2%~-1.5%，在噪声内；
  - `json_encode` 依赖数据形态：深嵌套 -11.55%、两层嵌套 -8.24%，
    普通记录 -3%~-5%，大 Unicode 字符串约 0%；
  - libxml2：DOM 解析 -2.12%、序列化 -3.61%，XPath 无稳定变化；
  - 代价：5 扩展树 38 MB → 61 MB；加 libxml2 后 46 MB → 71 MB；
    启动时间基本不变。
- **建议**：默认不要开；主要跑“小记录、深嵌套 JSON”或 DOM 解析/序列化、
  且能接受体积增长时再试。不要期待通用 Web/VM 负载加速。
- **下一步**：补 `libzip`、`libsodium` 的静态库和 LTO，再评估 PECL 扩展。

## 1. 目标与非目标

目标：

- 保持 `--static-all` 的全部行为：扩展和依赖链接进 `bin/php`，glibc 仍动态。
- PHP 本体的 C/C++ 代码加 `-flto=auto`。
- 依赖库也用 LTO 编译，静态库 `.a` 里保留 LTO 中间代码。
- 最终 `bin/php` 可以单独复制运行，不依赖 `deps/`。

非目标：

- 不静态链接 glibc；全静态 libc 的分析见 [STATIC-LIBC.md](STATIC-LIBC.md)。
- 不改变 `--static-all` 的语义；不带 LTO 时行为完全不变。
- 不保证所有依赖立刻支持 LTO。支持范围按第 7 节逐步扩大。
- 不承诺整体性能提升；LTO 的收益集中在库密集路径。

## 2. 与 `--static-all` 的区别

| 选项 | PHP 本体 | 依赖 | 产物 |
| --- | --- | --- | --- |
| `--static-all` | 普通优化 | 普通 `.a` | 单个 `bin/php` |
| `--static-all-lto` | `-flto=auto` | `.a` 也带 LTO | 单个 `bin/php`，依赖参与跨库优化 |

两者都会把扩展编入 PHP。`type=shared` 的扩展仍然自动排除。

## 3. 用法

```bash
# 最小集：PHP + openssl zlib curl sqlite oniguruma 全部 LTO
phprt install 8.5 --static-all-lto \
  --ext "openssl zlib curl sqlite mbstring"

# 只看计划
phprt install 8.5 --static-all-lto \
  --ext "openssl zlib curl sqlite mbstring" --plan

# 切回非 LTO 的单文件
phprt install 8.5 --static-all \
  --ext "openssl zlib curl sqlite mbstring"

# 切回普通安装
phprt install 8.5 --shared \
  --ext "openssl zlib curl sqlite mbstring"
```

规则：

- `--static-all-lto` 隐含 `--static-all`。
- 与 `--shared` 互斥。
- 与 `--build` 同时使用时，只接受 `名字=static`；出现 `名字=shared` 直接报错。
- `--static-all` 和 `--shared` 会显式关闭 LTO。
- `install` 不带链接参数时沿用树上记录。
- 状态：
  - `$TREE/.phprt-state/link`：`static` / `shared`。
  - `$TREE/.phprt-state/lto`：`1` / `0`。
- `phprt show` 显示 `链接方式: static（LTO）`。
- `--plan` 显示 `lto: 1`。

## 4. 开关

`bundle.conf` 里的 `LTO_PHP=0` 只作为默认值。运行时的优先级：

```text
PHPRT_LTO_<模块>  >  PHPRT_LTO  >  LTO_<模块>
```

- `--static-all-lto` 导出 `PHPRT_LTO=1`，同时作用于 `php` 和 `deps`。
- 只开 PHP 的旧实验方式仍可用：在 bundle 里设 `LTO_PHP=1`，不要设 `PHPRT_LTO`。
- `--static-all-lto` 不会写 bundle.conf；状态只记录在版本树里。

## 5. 依赖 LTO

### 5.1 保留原有优化级别

不能把依赖的 `CFLAGS` 统一覆盖成 `-flto=auto`，否则会丢掉各 recipe 原来的
`-O2` / `-O3`，LTO 的 A/B 结果不可比。v0 的做法是保留基线，再追加 LTO：

| 依赖 | 基线 | LTO 追加 |
| --- | --- | --- |
| openssl | `-Wall -O3` | `-flto=auto` |
| zlib | `-O3 -fPIC ...` | `-flto=auto` |
| curl | `-O2 ...` | `-flto=auto` |
| sqlite | `-O2 -fPIC -D...` | `-flto=auto` |
| oniguruma | `-g -O2` | `-flto=auto` |

### 5.2 静态库

LTO 后普通 `ar` 生成的归档可能缺少 LTO 符号索引。v0 用：

```text
AR=gcc-ar
RANLIB=gcc-ranlib
```

`gcc-ar` 会加载 `liblto_plugin.so`，把 LTO 符号写进归档索引。Makefile 里写死
`ar` 的构建系统（zlib、curl、oniguruma）需要显式传：

```bash
make AR=gcc-ar RANLIB=gcc-ranlib
```

直接调用编译器的 recipe（sqlite）改成使用 `$CC` 和 `$AR`。

如果某个构建系统不认 `gcc-ar`，回退方案是：

```text
-flto=auto -ffat-lto-objects
```

fat LTO 对象同时包含本机代码和 LTO 中间代码，普通 `ar` 也能生成可链接的
归档；最终链接仍会走 LTO。

### 5.3 构建后校验

每个依赖构建完成后，如果 LTO 开启，检查它的静态库：

```bash
readelf -S deps/<名字>/lib/*.a | grep -q '\.gnu\.lto'
```

没有 LTO 段就失败，提示：

```text
依赖 X 的静态库没有 LTO 段（recipe 未接 LTO）
```

这样可以避免 recipe 悄悄按非 LTO 构建，却把结果记成 LTO 树。

## 6. 签名与重编

- `dep_sig()` 包含：
  - `lto=1/0`
  - `lto_flags`
- `PHP_SIG` 包含：
  - `lto_php`
  - `lto_deps`
- 因此以下切换都会重编：
  - `--static-all` ↔ `--static-all-lto`
  - 修改 `LTO_CFLAGS` / `LTO_LDFLAGS`
  - 依赖 LTO 状态变化
- `php.sh` 的 `.phprt-build-flags` 已经包含 `CFLAGS` / `LDFLAGS` / LTO 链接参数，
  非 LTO 对象不会和 LTO 对象混用。

## 7. 支持范围

| 阶段 | 范围 | 状态 |
| --- | --- | --- |
| v0 | `openssl zlib curl sqlite oniguruma` | 已实现（2026-11-09） |
| v1 | `libxml2` 已接入静态库和 LTO；`libzip`、`libsodium` 待做 | 部分完成（2026-11-09） |
| v2 | 其余 autotools 依赖 | 待做 |
| v3 | CMake 类依赖（aom/avif/jpeg/libzip/tidy/webp 等） | 待做 |
| v4 | 需要额外静态库或 C++ 运行库的依赖 | 待做 |

`--static-all-lto` 的依赖 LTO 覆盖范围与 `--static-all` 的 `.a` 覆盖范围一致：
没有 `.a` 的依赖本来就不能用于 `--static-all`。

## 8. 构建机制

PHP 侧沿用 `--static-all` 的做法：

1. 只含依赖 `.a` 的 `static-libs/` 搜索目录放在所有 `-L` 前面；
2. 删掉 configure 生成的依赖 rpath；
3. `EXTRA_LIBS` 重复两遍，解决静态库单向扫描顺序；
4. 加 `-static-libgcc -static-libstdc++`；
5. 两个全局寄存器文件继续用 `-fno-lto`：
   - `Zend/zend_execute.c`
   - `ext/opcache/jit/zend_jit_vm_helpers.c`

LTO 只增加 `-flto=auto`，不改变静态链接的库选择。

构建完成后检查：

- `bin/php` 的 `NEEDED` 只允许 glibc；
- `bin/php` 不残留 `.gnu.lto_*` 段；
- 只存在相对 `$ORIGIN` RPATH。

## 9. 验收

```bash
cd /path/to/project
phprt install 8.5 --static-all-lto \
  --ext "openssl zlib curl sqlite mbstring"
phprt show
phprt verify
```

| 检查 | 预期 |
| --- | --- |
| 构建 | `BUILD-STATUS=ok` |
| 链接方式 | `.phprt-state/link` 为 `static` |
| LTO | `.phprt-state/lto` 为 `1` |
| `BUILD-INFO` | `lto_php=1`、`lto_deps=1` |
| 依赖日志 | 出现 `-flto=auto`；libtool 静默时以 `.a` 的段为准 |
| 依赖 `.a` | 存在 `.gnu.lto` 段 |
| `ldd bin/php` | 只有 glibc 和动态加载器 |
| `readelf -S bin/php` | 没有 `.gnu.lto` 残留 |
| 单文件 | 复制到空目录后 `php -v` / `php -m` 正常 |
| 功能 | sqlite、mbstring、openssl、curl 正常 |
| `phprt verify` | 全绿 |

性能对比用同一组扩展的非 LTO 树（例如 `v2sa`）：

```bash
taskset -c 2 <tree>/bin/php -n -d memory_limit=256M Zend/bench.php
```

至少比较：

- `bin/php` 体积；
- 启动时间；
- `Zend/bench.php` / `micro_bench.php`；
- JSON 编解码；
- mbstring；
- PCRE（`pcre.jit=0`）；
- zlib 压缩；
- OpenSSL AES / hash；
- SQLite 内存库。

LTO 的整体收益通常很小；重点看依赖 LTO 是否让 zlib、OpenSSL、curl、sqlite
这类库密集路径出现稳定变化。

## 10. 风险与回退

- LTO + 静态 OpenSSL/curl 可能暴露符号顺序或链接错误；先保留日志，必要时
  单个依赖回退，或改用 `-ffat-lto-objects`。
- `gcc-ar` 不被某个构建系统尊重时，用 `make AR=... RANLIB=...` 显式传。
- LTO 会明显增加链接时间和内存；属于预期。
- `phpdbg` 或某个静态库单独失败时，先缩小到 CLI 验收，再决定是否限制范围。
- 如果组合无法稳定链接，`--static-all` 保持原样，`--static-all-lto` 标记为
  实验性，不阻塞现有功能。

## 11. 与 v1 LTO 的关系

v1 的 `phprt/docs/LTO.md` 只验证了 PHP 本体 LTO，依赖和 PECL 没接。结论是：

- 整体 VM 负载基本 ±1%；
- JSON / mbstring / PCRE 等编译进 PHP 的库有 4%–13% 的局部收益；
- 依赖库不是 LTO，所以 zlib、Oniguruma、ICU 没有一致变化。

`--static-all-lto` 把依赖也编进同一个链接单元，重点验证 v1 没覆盖的部分。
