# 用安装树跑 PHP 自带用例

1. 范围：直接调用安装好的 `bin/php` 跑 PHP 源码里的 `.phpt` 用例，不触发重新编译。
   可以验证扩展是否真的能用，也能发现打包问题（缺库、缺 CA、缺子命令）。

2. 前置条件：
   - 安装树存在：`.phprt/php-<版本>/bin/php`
   - PHP 源码树存在：`.phprt/build/php-8.5.12/`（`run-tests.php` 在它根目录）
   - 源码树由构建流程解压而来；没有就先跑一次 `phprt install 8.5`，不要另下一份源码

3. 在项目根目录执行（冒烟档，约 12k 个用例，24 并发约 50 秒）：

   ```bash
   TREE="$(readlink -f .phprt/current)"
   SRC="$(readlink -f .phprt/build/php-8.5.12)"
   EXT="$("$TREE/bin/php-config" --extension-dir)"

   cd "$SRC" &&
   TEST_PHP_EXECUTABLE="$TREE/bin/php" TEST_PHP_SRCDIR="$SRC" NO_INTERACTION=1 \
   "$TREE/bin/php" -n run-tests.php -n \
     -d extension_dir="$EXT" \
     -d extension=sqlite3 -d extension=pdo_sqlite -d extension=zlib \
     -j24 -w failed.list \
     Zend/tests tests ext/standard/tests ext/openssl/tests ext/curl/tests \
     ext/intl/tests ext/mbstring/tests ext/zip/tests ext/sockets/tests ext/gd/tests \
     ext/sodium/tests
   ```

4. 不要用 `make test`：
   - 它会按 Makefile 重新编译，编译参数和安装树不一致
   - 构建树里的 `sapi/cli/php` 带构建期 RUNPATH，脱离 `deps/` 就跑不起来
     （`libicudata.so.79: cannot open shared object file`）
   - 安装树的二进制已经 wheel 化，RPATH 是 `$ORIGIN`，搬到哪都能跑

5. 参数说明：
   - 外层 `-n`：不读树内 `php.ini`／`conf.d`，避免被测配置影响结果
   - `run-tests.php -n`：让每个用例进程也不读 ini
   - `-d extension_dir` 与 `-d extension=...`：显式加载用例需要的扩展
   - `-w failed.list`：把失败用例的绝对路径写下来，便于重跑
   - 重跑失败用例：把 `-j24 -w ...` 换成 `-j4 -r failed.list`

6. 结果解读：
   - `Tests skipped`：缺少依赖或平台不支持（MySQL、ZTS、debug 构建、网络等）
   - `Expected fail`（XFAIL）：用例本来就标注了已知问题，不算失败
   - `Tests failed`：真失败，看同目录的 `.diff`（期望 vs 实际）

7. 当前 8.5.12 的已知差异（ICU 79.1 的 CLDR 数据变化，不是打包问题）：
   - `ext/intl/tests/dateformat_formatObject_calendar_variant_icu72-1.phpt`
   - `ext/intl/tests/dateformat_formatObject_datetime_variant_icu72-1.phpt`
   - `ext/intl/tests/timezone_IDforWindowsID_basic_icu76_1.phpt`

   差异内容是 `GMT+03:00` 变成 `+03:00 GMT`、`America/Vancouver` 变成
   `America/Los_Angeles`。这些用例的 `--SKIPIF--` 只判 `>= 72.1`，没有覆盖 79。

8. 清理产物：失败用例会在源码目录留 `.diff`、`.out`、`.exp`、`.log`、`.php`、`.sh`。
   按 `failed.list` 里的名字逐个删掉；`failed.list` 本身也可以删。
   源码目录是构建产物，不要把它提交进仓库。
