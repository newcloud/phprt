# PHP 8.5.12 的 LTO 构建

- 状态：仅 PHP 本体可用，依赖库和 PECL 扩展未接；默认关闭（`LTO_PHP=0`），需要时手动打开
- 日期：2026-11-05（2026-11-06 收尾：实验结束，补丁方案放弃，默认值改回 0）
- 适用：`bundles/php-8.5.12`
- 相关：`docs/TESTING.md`、`docs/RUNTIME-LIB-VERSIONING.md`

## 1. 范围

只有 PHP 8.5.12 本体的编译加 LTO：

```text
php.sh -> CFLAGS/CXXFLAGS -> -O2 [-flto=auto]
      -> LDFLAGS          ->      [-flto=auto]
```

不动的部分：

- `deps/*` 里的 OpenSSL、zlib、libxml2、curl、sqlite、libzip 等
- 用 phpize 单独编的 PECL 扩展（redis、protobuf、xdebug、apcu、yaml、swoole）

PHP 构建时顺带编出的共享扩展（zlib、sqlite3、pdo_sqlite、opcache 等）会跟着 PHP 的
CFLAGS 一起带 LTO，但它们各自是独立链接单元，不和其他 `.so` 做跨库优化。

## 2. 开关

`bundles/php-8.5.12/bundle.conf`：

```bash
LTO_PHP=0                 # 0 = 非 LTO 对照；1 = LTO 版本
PHP_CFLAGS="-O2"          # 基础参数，两个版本共用，不带 -g
PHP_CXXFLAGS="-O2"
```

`runner/common.sh`：

```bash
LTO_CFLAGS="-flto=auto"   # 只在 LTO_PHP=1 时追加
LTO_LDFLAGS="-flto=auto"
```

相关函数：

```bash
lto_on php     # 读 LTO_PHP
lto_state php  # 输出 0/1，用于签名
```

### 为什么不用默认的 -g

PHP 的 autoconf 默认是 `-g -O2`。为了做「只差 LTO」的对比，这里显式设置
`PHP_CFLAGS="-O2"`，两个版本都不带 `-g`。需要调试符号时改成 `"-O2 -g"`。

### 动态库的限制

LTO 不能跨动态库边界。`libssl.so`、`libcurl.so`、`swoole.so` 各自独立，
只能在自己的链接单元内优化。所以这一轮只开 PHP，收益集中在 PHP 引擎和
PHP 自带的静态代码路径上。

### 全局寄存器文件例外

PHP 有两个文件使用 GCC 全局寄存器变量（`%r14` / `%r15`）：

```text
Zend/zend_execute.c                    — VM 执行器
ext/opcache/jit/zend_jit_vm_helpers.c  — JIT helper
```

LTO 把多个翻译单元合并后，GCC 会报：

```text
global register variable follows a function definition
```

处理方式：只有这两个文件用 `-fno-lto` 编译，其余 PHP 对象仍然参与 LTO。
`php.sh` 在 configure 之后修改生成的 `Makefile`，并删掉它们的旧对象：

```bash
sed -i '/-c .*\/zend_execute\.c/ s|\$(CFLAGS_CLEAN)|$(CFLAGS_CLEAN) -fno-lto|' Makefile
sed -i '/-c .*\/zend_jit_vm_helpers\.c/ s|\$(CFLAGS_CLEAN)|$(CFLAGS_CLEAN) -fno-lto|' Makefile
rm -f Zend/zend_execute.lo Zend/zend_execute.dep Zend/.libs/zend_execute.o \
      ext/opcache/jit/zend_jit_vm_helpers.lo \
      ext/opcache/jit/zend_jit_vm_helpers.dep \
      ext/opcache/jit/.libs/zend_jit_vm_helpers.o
```

这不会改变运行时行为：全局寄存器仍然保留，JIT 也仍然可用；只是这两个
目标文件不参与 LTO。代价是 VM 执行器本身没有跨文件 LTO 优化。

## 3. 签名与重编

- `runner/build.sh` 的 `PHP_SIG` 包含 `PHP_CFLAGS`、`PHP_CXXFLAGS` 和 LTO 状态。
- 修改 `LTO_PHP` 或 `PHP_CFLAGS` 后必须用 `phprt set <版本> --recompile` 才会重编
  （已完成的树在普通 `phprt set` 下只切换，不走构建）。
- `php.sh` 在源码树里记录 `.phprt-build-flags`。CFLAGS/CXXFLAGS 或 LTO 链接参数
  变化时先 `make distclean`，避免旧的 `-O2` 对象和新的 LTO 对象混用。
- 签名只记录影响代码生成的参数，不记录依赖的 `-L/-rpath`；依赖升级不会因此
  触发 PHP 全量清理重编。

## 4. 构建

```bash
cd /home/z/testphp/blog

# 非 LTO 对照
#   bundle.conf: LTO_PHP=0
.phprt/bin/phprt set 8.5.12 --recompile

# 复制对照树（.phprt-bench/ 是临时目录，实验结束后已删除）
mkdir -p .phprt-bench
cp -a .phprt/php-8.5.12 .phprt-bench/php-8.5.12-nolto

# LTO 版本
#   bundle.conf: LTO_PHP=1
.phprt/bin/phprt set 8.5.12 --recompile
```

确认两个版本都没有 DWARF 调试段：

```bash
readelf -S .phprt-bench/php-8.5.12-nolto/bin/php | grep -c '\.debug_info'
readelf -S .phprt/php-8.5.12/bin/php            | grep -c '\.debug_info'
```

期望都是 `0`。`.debug_gdb_scripts` 是 PHP 自带的 GDB 辅助段，不受 `-g` 影响，
它的存在不代表编译时带了调试信息。

确认 LTO 进了编译：

```bash
grep -m3 -- '-flto' .phprt/logs/php-make.log
grep -m1 'lto_php=' .phprt/php-8.5.12/BUILD-INFO
```

## 5. 性能测试

### 官方脚本

php-src 自带（当前源码树里）：

```text
.phprt/build/php-8.5.12/Zend/bench.php
.phprt/build/php-8.5.12/Zend/micro_bench.php
.phprt/build/php-8.5.12/ext/hash/bench.php
```

php-src 的 `real-time-benchmark` workflow 也使用 `Zend/bench.php`，可选
`Zend/micro_bench.php`，并分别开/关 opcache 和 JIT。

`benchmark/benchmark.php` 不直接使用：它要求 mbstring、sockets、mysqli、openssl、
gmp，还要 php-cgi；当前树没有 gmp，且是 `--disable-cgi`。

运行方式（同一份脚本，分别用两个树）：

```bash
SRC=/home/z/testphp/blog/.phprt/build/php-8.5.12

taskset -c 2 .phprt-bench/php-8.5.12-nolto/bin/php -n -d memory_limit=256M \
  "$SRC/Zend/bench.php"
taskset -c 2 .phprt/php-8.5.12/bin/php -n -d memory_limit=256M \
  "$SRC/Zend/bench.php"
```

`micro_bench.php` 同样跑；再各跑一组 opcache/JIT：

```bash
-d opcache.enable_cli=1 -d opcache.jit=off
-d opcache.enable_cli=1 -d opcache.jit=tracing
```

### blog 实际负载

- 进程内：Slim 处理 `GET /`（SQLite 查询 + 模板渲染）
- HTTP：RoadRunner + `ab -k`，分别测 `GET /` 和 `/health`

临时配置和脚本放 `/tmp/opencode/lto-bench/`，不改 blog 的 `.rr.yaml`。

### 方法

- `taskset -c 2` 固定单核（HTTP 除外）
- 预热后 A/B 交替，多次取中位数
- 记录 median、最小值、相对变化
- 同时对比 `bin/php` 体积和启动 RSS

## 6. 实测结果

环境：GCC 15.3.1、24 核、31G 内存；2026-11-05。
两个树都是 `-O2`、不带 `-g`（对照树是临时目录，已随实验结束删除）：

```text
.phprt-bench/php-8.5.12-nolto   非 LTO 对照
.phprt/php-8.5.12               LTO（-flto=auto，两个全局寄存器文件除外）
```

Smoke（`docs/TESTING.md` 的冒烟档）：两个树都是 12119 个用例、3 个失败，
失败列表完全一致，都是 ICU 版本差异，没有 LTO 引入的回归。

### CLI / 官方脚本

`taskset -c 2`，A/B 交替，取中位数：

| 用例 | 非 LTO | LTO | 变化 |
| --- | ---: | ---: | ---: |
| 启动 `php -r ';'` | 0.0066s | 0.0066s | -0.82% |
| `Zend/bench.php` | 0.1805s | 0.1798s | -0.40% |
| `Zend/bench.php` + opcache | 0.1182s | 0.1188s | +0.50% |
| `Zend/bench.php` + opcache + JIT tracing | 0.0495s | 0.0498s | +0.75% |
| `Zend/micro_bench.php` | 0.7383s | 0.7420s | +0.50% |
| JSON 编解码 | 0.6411s | 0.6277s | -2.10% |
| SQLite 内存库 | 0.0744s | 0.0757s | +1.79% |
| blog Slim `GET /`（进程内） | 0.0605s | 0.0599s | -1.05% |

`Zend/bench.php` 单项输出只有 1ms 精度，单项差异基本是量化噪声；总和：

- `Zend/bench.php`：0.171s → 0.174s（+1.75%）
- `Zend/bench.php` + opcache：0.109s → 0.109s（0.00%）
- `Zend/micro_bench.php`：0.416s → 0.422s（+1.44%）

### HTTP（RoadRunner + ab）

4 个 worker，`ab -k -c 16`，每边 3 轮取中位数：

| 路径 | 非 LTO | LTO | 变化 |
| --- | ---: | ---: | ---: |
| `GET /` | 17621.7 req/s | 17805.7 req/s | +1.05% |
| `GET /health` | 18254.4 req/s | 18353.7 req/s | +0.54% |

### 库级别基准

同样的两个树，`taskset -c 2`，A/B 交替 7 轮，脚本内部 `hrtime` 计时。
noise 是本轮极差（max-min / median），差值落在 noise 内视为无差异。

| 组件 | 参与 LTO | 用例 | 非 LTO | LTO | 变化 |
| --- | --- | --- | ---: | ---: | ---: |
| PCRE2（`pcre.jit=0`） | 是 | email_match_all | 2.1458s | 2.0518s | **-4.38%** |
| PCRE2（`pcre.jit=0`） | 是 | callback_replace | 0.2323s | 0.2244s | **-3.38%** |
| PCRE2（`pcre.jit=0`） | 是 | date_match_all | 1.2607s | 1.2423s | -1.46%（噪声内） |
| PCRE2（`pcre.jit=0`） | 是 | tag_replace | 0.6094s | 0.6215s | +1.99%（噪声内） |
| PCRE2（`pcre.jit=0`） | 是 | word_split | 1.1123s | 1.1050s | -0.66%（噪声内） |
| PCRE2（`pcre.jit=1`） | 是 | email_match_all | 0.1862s | 0.1855s | -0.37%（噪声内） |
| PCRE2（`pcre.jit=1`） | 是 | callback_replace | 0.1318s | 0.1277s | -3.11%（边界） |
| PCRE2（`pcre.jit=1`） | 是 | word_split | 0.4755s | 0.4945s | +4.00%（噪声内） |
| Oniguruma（`mb_ereg`） | 否 | mb_ereg_match | 0.1308s | 0.1293s | -1.20%（噪声内） |
| Oniguruma（`mb_ereg`） | 否 | mb_ereg_replace | 0.3873s | 0.3809s | -1.65%（噪声内） |
| Oniguruma（`mb_ereg`） | 否 | mb_ereg_capture | 0.0712s | 0.0692s | -2.74%（噪声内） |
| mbstring | 是 | mb_strtolower | 1.3910s | 1.2097s | **-13.03%** |
| mbstring | 是 | mb_substr | 0.1789s | 0.1678s | **-6.20%** |
| mbstring | 是 | mb_strlen | 0.5277s | 0.5088s | -3.59%（边界） |
| mbstring | 是 | mb_convert_encoding | 0.2177s | 0.2083s | -4.32%（边界） |
| zlib（`gzcompress`） | 否 | compress | 0.4680s | 0.4680s | 0.00% |
| zlib（`gzuncompress`） | 否 | uncompress | 0.1982s | 0.1968s | -0.71% |
| zlib（`gzdeflate`） | 否 | deflate | 0.4252s | 0.4253s | 0.02% |
| ICU（`intl`） | 否 | number_format | 0.3066s | 0.3027s | -1.29%（噪声内） |
| ICU（`intl`） | 否 | collator_compare | 0.0829s | 0.0839s | +1.28%（噪声内） |
| ICU（`intl`） | 否 | date_format | 0.7238s | 0.7287s | +0.69%（噪声内） |
| JSON | 是 | json_encode | 0.2038s | 0.1808s | **-11.30%** |
| JSON | 是 | json_decode | 0.8685s | 0.8549s | -1.57%（噪声内） |
| hash | 是 | sha256（16MB） | 0.7384s | 0.7375s | -0.12%（噪声内） |
| hash | 是 | crc32b（16MB） | 0.2440s | 0.2410s | -1.25%（噪声内） |
| serialize | 是 | serialize | 0.2547s | 0.2621s | +2.90%（噪声内） |
| serialize | 是 | unserialize | 0.3395s | 0.3320s | -2.21%（噪声内） |

zlib 首轮中位数受 `powersave` governor 的频率抖动影响，出现 +10% 的假差异；
加跑 12 轮后 min/median 差异都在 ±1% 内，上表用的是加跑结果。

JSON 这一行的 -11.30% 是"1000 条同构记录"的简单形态，不能当通用结论；
复杂负载的复核见下一节。

### 反向优化实验：不用 LTO 复刻 json_encode 收益

> 状态（2026-11-06）：本节及下一节的实验已结束。结论是不并入 phprt：
> `smart_str` 补丁只对"小值高频追加"的形态有效、要改公共头文件，`restrict` 无收益。
> 源码已还原为 stock，`.phprt-bench/` 对比树已删除，记录保留仅供追溯。

机器码里能看到几处 LTO 变换，于是拆成三个隔离版本（都是非 LTO、无 `-g`）：

- `smartstr`：只改 `Zend/zend_smart_str.h`，非 fallible 路径拆成持久/非持久
  特化入口（`smart_str_realloc` / `smart_str_erealloc`），绕开通用
  `smart_str_ferealloc` 的 fallible 分支。
- `restrict`：只给 `ext/json/json_encoder.c` 的三个热函数指针参数加 `restrict`。
- `both`：两者都加。

补丁树：`.phprt-bench/php-8.5.12-smartstr`、`...-restrict`、`...-json-patch`。

| 用例 | stock | smartstr | restrict | both | LTO |
| --- | ---: | ---: | ---: | ---: | ---: |
| `nested` | 0.2053s | 0.1876s (-8.64%) | 0.2055s (+0.10%) | 0.1900s (-7.48%) | 0.1841s (-10.34%) |
| `objects` | 0.1079s | 0.0974s (-9.78%) | 0.1095s (+1.43%) | 0.0973s (-9.82%) | 0.1024s (-5.10%) |
| `array_assoc` | 0.0364s | 0.0344s (-5.50%) | 0.0360s (-1.04%) | 0.0343s (-5.83%) | 0.0342s (-5.99%) |
| `array_list` | 0.0195s | 0.0192s (-1.48%) | 0.0198s (+1.42%) | 0.0192s (-1.89%) | 0.0188s (-3.51%) |
| `long_escape` | 0.8707s | 0.8519s (-2.16%) | 0.8686s (-0.24%) | 0.8485s (-2.55%) | 0.7327s (-15.84%) |
| `long_unicode` | 0.1425s | 0.1427s (+0.11%) | 0.1533s (+7.52%) | 0.1425s (-0.02%) | 0.1377s (-3.43%) |
| `mb_strtolower` | 1.3839s | 1.3953s (+0.82%) | 1.3806s (-0.24%) | 1.3914s (+0.54%) | 1.2283s (-11.24%) |

结论：

- **smartstr 是唯一可复刻的收益来源**：`nested` -8.64%、`objects` -9.78%、
  `array_assoc` -5.50%，和 LTO 的 -10.34% / -5.10% / -5.99% 同一量级。
- **restrict 单独没有收益，甚至会变慢**：`long_unicode` +7.52%、
  `array_list` +1.42%；`both` 和 `smartstr` 基本一样，说明 restrict 没有贡献。
- `long_escape`：LTO -15.84%，`smartstr` 只有 -2.16%，`both` -2.55% →
  复刻不了，更像 LTO 重排后的分支布局/代码对齐效应。
- `mb_strtolower`：LTO -11.24%，三个补丁都在 ±1% → 不是 smart_str 能解释的。
- 第一版 `smartstr` 曾把非持久路径也换成持久分配器，导致
  `zend_mm_heap corrupted`；必须严格区分 persistent / non-persistent。

因此：

- 作为上游候选，`smart_str` 非 fallible 特化值得单独提，但需要更广的基准和
  严格的持久/非持久测试；`restrict` 不建议加。
- 作为 phprt 本地补丁，不值得保留：改的是 PHP 核心头文件，影响所有扩展，
  收益集中在 JSON 数组/对象路径，对典型 Web 应用可以忽略。

### LTO + smart_str 叠加，以及复杂 JSON 复核

前面的 -11.3% / -8.6% / -9.8% 都来自"同构记录"的简单形态。为了确认它不是
微基准假象，补了一组更接近真实业务的负载（0.1–3.9MB，含深嵌套、Unicode、
重转义、浮点矩阵、null 密集对象），并加编了一棵 LTO + smart_str 的树
（`.phprt-bench/php-8.5.12-lto-smartstr`）。

这棵树的调用形态和前三棵都不同：LTO 把特化后的调用进一步内联，`php_json_encode_array`
里出现 31 处直接 `call _erealloc2`、19 处 `smart_str_erealloc.part.0`、7 处
`smart_str_erealloc`，而 stock LTO 是 54 处 `call smart_str_ferealloc`。

11 轮、A/B 轮转、`taskset -c 2`，中位数：

| 用例 | 文档大小 | 非 LTO | smart_str | LTO | LTO + smart_str |
| --- | ---: | ---: | ---: | ---: | ---: |
| `api_page` | 0.73MB | 0.0806s | 0.0784s (-2.73%) | 0.0781s (-3.10%) | 0.0793s (-1.61%) |
| `deep_tree` | 0.11MB | 0.0492s | 0.0450s (-8.54%) | 0.0441s (-10.37%) | 0.0473s (-3.86%) |
| `html_list` | 0.12MB | 0.0629s | 0.0639s (+1.59%) | 0.0629s (0.00%) | 0.0626s (-0.48%) |
| `unicode_mix` | 0.53MB | 0.0750s | 0.0724s (-3.47%) | 0.0739s (-1.47%) | 0.0758s (+1.07%) |
| `float_matrix` | 0.66MB | 0.2499s | 0.2511s (+0.48%) | 0.2501s (+0.08%) | 0.2507s (+0.32%) |
| `sparse_objects` | 0.20MB | 0.0718s | 0.0651s (-9.33%) | 0.0676s (-5.85%) | 0.0739s (+2.92%) |
| `big_document` | 3.88MB | 0.0883s | 0.0856s (-3.06%) | 0.0897s (+1.59%) | 0.0891s (+0.91%) |
| `log_lines` | 0.74MB | 0.0965s | 0.0978s (+1.35%) | 0.1000s (+3.63%) | 0.0989s (+2.49%) |
| `decode_api`（对照） | 0.73MB | 0.0950s | 0.0955s (+0.53%) | 0.0936s (-1.47%) | 0.0933s (-1.79%) |

结论：

- 收益随"每条记录越小、追加次数越多"上升。`deep_tree` -8.5%、`sparse_objects` -9.3%，
  这两者都是几十万个小字符串/小整数拼出来的文档。
- 真实体量的结构化负载只剩 -1.6%~-3.5%（`api_page`、`unicode_mix`、`big_document`）。
  `float_matrix`、`html_list`、`log_lines` 在噪声内或略微变慢，因为它们分别由
  数字格式化、转义、`memcpy` 支配，分配路径不是瓶颈。
- **LTO 和补丁不叠加，甚至会互相干扰。** `deep_tree`：单独 LTO -10.4% > 补丁 -8.5% > 两者 -3.9%；
  `sparse_objects`：补丁 -9.3% > LTO -5.9% > 两者 +2.9%。两者都在改同一批调用的形态和
  代码布局，同时开启会互相覆盖。
- 所以 -11.3% 只适用于简单同构数组；复杂负载的真实收益是 0~3%，个别"小值高频追加"
  的形态能到 8~9%。
- `json_decode` 无变化，符合预期：补丁只碰编码侧的 `smart_str` 追加路径。

### 结论

要分两类看：

**VM / 用户态 PHP 代码：基本没有收益。**

- 官方 `Zend/bench.php`、`micro_bench.php`、blog 进程内和 RoadRunner HTTP
  都在 ±1% 左右。
- 原因是 `Zend/zend_execute.c`（VM 执行器）必须排除出 LTO（全局寄存器变量），
  这些负载主要跑在解释执行路径上。
- 其余 PHP 代码和静态扩展确实参与了 LTO（`php-make.log` 中 `-flto=auto`
  出现 635 次），但不足以改变整体结果。

**库级别 C 代码：只有编译在 PHP 构建内部的库有稳定收益。**

- PCRE2（`preg_*`，`pcre.jit=0`）：`email_match_all` -4.4%、
  `callback_replace` -3.4%。开启 PCRE 自带的 JIT 后收益基本消失，
  因为 JIT 生成的代码不受 LTO 影响。
- mbstring：`mb_strtolower` -13.0%、`mb_substr` -6.2%，
  `mb_strlen` / `mb_convert_encoding` 约 -4%（接近噪声带）。
- JSON：`json_encode` 在简单同构数组上 -11.3%，复杂负载只有 -1.6%~-3.5%，
  少数"小值高频追加"的形态能到 -8%~-9%；`json_decode`、serialize、hash 基本不变。
  其中一部分可以用 `smart_str` 非 fallible 快路径复刻，但 LTO + 补丁不叠加，
  详见反向优化实验和复杂负载复核两节。**补丁未并入 phprt**：收益面太窄，
  代价是改公共头文件。
- 外部库 Oniguruma（`mb_ereg`）、ICU（`intl`）、zlib 没有一致变化，
  因为它们不是用 LTO 编译的。
- hash 使用高度优化的实现/汇编，LTO 没有额外收益。

所以 LTO 不是“整体加速 PHP”，而是“加速那些恰好编译进 PHP 构建、又没有被
JIT 或汇编替代的 C 路径”。对典型 Web 应用（VM 为主）不值得；对 JSON、
mbstring、PCRE 解释器这类库密集型负载，能看到 4%–13% 的局部收益。

两个版本的 `bin/php` 都是 29M，都没有 `.debug_info`；LTO 没有带来体积变化。
保留开关是为了继续做对比实验；要回到 stock 构建，把 `bundle.conf` 的
`LTO_PHP` 改回 0 后 `phprt set 8.5.12 --recompile`。

如果想进一步验证“完整 LTO”的潜力，需要同时关闭 GCC global registers
（`--disable-gcc-global-regs`）让 `zend_execute.c` 也参与 LTO；但那会改变
PHP 默认的 VM 优化方式，必须两个对比版本一起关，属于另一组实验，当前未做。

原始输出原本在 `/tmp/opencode/lto-bench/`（临时目录，已被系统清理，未保留）：
`results-cli.txt`、`results-http.txt`、`results-lib.txt`、`raw/`、`raw-lib/`。
本文表格里的数字是这些输出的中位数汇总。

## 7. 回滚

```bash
cd /home/z/testphp/blog

# bundle.conf: LTO_PHP=0
.phprt/bin/phprt set 8.5.12 --recompile
```

实验期间的 `.phprt-backup/php-8.5.12-pre-lto-20261105` 快照已随实验结束删除，
回滚走上面这条重编路径，不要再依赖该快照。
