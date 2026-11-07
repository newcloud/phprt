# 运行时库 wheel 与 OpenSSL 依赖边界

- 状态：记录当前实现和结论
- 日期：2026-11-05
- 适用：phprt v2（项目本地自包含模型）

## 1. 当前 wheel 做什么

依赖库安装在：

```text
<tree>/deps/<库>-<版本>/lib
```

构建收尾时 `wheelize_tree` 把共享库平铺到 `<tree>/lib/phprt`：

- 库名换成稳定的运行时名（rt + SONAME 的 ABI 版本号）；
- 库自己的 SONAME 改成运行时名；
- `bin/php`、扩展和其他库的 `DT_NEEDED` 改到新名字；
- 所有 ELF 的 RPATH 改成 `$ORIGIN` 相对路径。

示例：

```text
libssl.so.3      → libssl-rt3.so.3
libcrypto.so.3   → libcrypto-rt3.so.3
libz.so.1        → libz-rt1.so.1
```

目标只有三个：

1. 不加载系统上的同名动态库；
2. 整棵树可以改名或搬家；
3. 运行时不依赖 `LD_LIBRARY_PATH`。

运行时名只表达 ABI 系列，不表达内容：同系列小版本升级名字不变，完整性由
`wheel.map` 的 sha256 列和 `phprt verify` 负责。rt 名与构建目录、编译选项
无关，跨目录构建名字稳定。

增量构建时 `wheel.map.prev` 保存上一轮映射，跨系列改名后把消费方里的旧
运行时名改成新名字；同系列升级映射为空，零改写。

## 2. 依赖更新是否联动

版本上互不绑定；是否重编由依赖签名决定。签名里参与判断的是依赖的
「稳定 ABI 身份」（它装的所有库的原始 SONAME 集合，`dep_rt_id`），不是
版本号也不是目录路径：同系列小版本升级身份不变，消费方不重编。

| 更新对象 | 其他组件版本要跟着变吗 | 当前签名策略下会重编 |
| --- | --- | --- |
| swoole | 不用 | 只 swoole |
| curl | 不用 | curl + PHP（curl 是内建扩展） |
| libzip | 不用 | libzip + PHP（zip 是内建扩展） |
| openssl | 不用 | 只 openssl（`phprt update`，见第 9 节） |

openssl 那一行靠 `phprt update` 实现：同系列小版本升级只重编 openssl、
替换运行时库，消费方签名不变、零重编。跨系列升级（SONAME 变化）会被
`phprt update` 拒绝，走 `phprt install --recompile` 的正常重编路径。

更新一个组件不会自动升级它的兄弟组件；只会让依赖它的组件重新编译。

## 3. PHP / Swoole / OpenSSL 的加载关系

当前树中：

```text
readelf -d swoole.so    → 没有 libssl / libcrypto 的 NEEDED
nm -D swoole.so         → U SSL_CTX_new
readelf -V swoole.so    → 没有 OPENSSL_* 符号版本要求
```

启动时：

```text
LD_DEBUG=libs → 只加载一份 libssl-rt3.so.3
               libssl 先初始化，swoole.so 后初始化
```

结论：**swoole 没有自己的 OpenSSL 实例，它使用 PHP 已经加载的那份。** 当前进程里只有一份 OpenSSL。

PHP 的 `bin/php` 对 libssl 的引用是带版本的：

```text
SSL_CTX_new@OPENSSL_3.0.0
```

swoole 的引用是不带版本的 `SSL_CTX_new`。

## 4. 强制加载第二份 OpenSSL 会怎样

如果让 swoole 单独链接另一个 OpenSSL，而 PHP 链接当前版本：

- PHP 的 openssl 扩展编在 `bin/php` 里，启动时绑定已经完成；后面加载的库不会替换 PHP 的绑定。
- PHP 加载扩展使用 `RTLD_GLOBAL`，符号查找先查全局作用域。PHP 的 libssl 在启动时就已加载，排在前面。
- swoole 的未版本化引用通常会先命中 PHP 的 libssl，而不是 swoole 自己链接的那份。
- 如果通过 `RTLD_DEEPBIND`、静态链接或 `dlopen` + `dlsym` 强制使用自己的版本，就会变成两份 OpenSSL：符号插入、全局状态分裂、对象不能跨库传递。
- PHP 的 Apache SAPI 会开 deepbind；CLI/FPM 默认不开，不同 SAPI 行为还会不一致。

因此：**一棵 PHP 树、一个 PHP 进程只允许一个 OpenSSL 版本。**

## 5. 静态链接 OpenSSL 的情况

如果 swoole 静态链接 OpenSSL，PHP 动态链接另一个补丁版本：

- 静态库的公共符号是 `GLOBAL DEFAULT`，swoole 的链接行默认没有 `-Wl,-Bsymbolic`、`--exclude-libs,ALL` 或 version script，所以 `swoole.so` 会导出 `SSL_CTX_new` 等定义。
- ELF 允许多个同名定义，不会在加载时报“符号重复”。
- PHP 的 libssl 先加载，全局查找优先命中它；swoole 的公共 API 调用可能被“插入”到 PHP 的 libssl，而 swoole 里静态编入的内部函数和全局变量仍然存在，形成混合调用。
- 如果 swoole 加了 `-Bsymbolic` 或隐藏符号，swoole 用自己的静态副本，PHP 用动态副本；这是两套 OpenSSL 状态，安全补丁会被绕过。
- `SSL_CTX`、`X509`、`EVP_PKEY` 等对象跨两份库传递是未定义行为，即使 patch 版本 ABI 兼容也不应依赖。

OpenSSL 自己的 `libssl.so.3` 链接时用了 `-Wl,-Bsymbolic`，共享库内部调用是自洽的；静态库被链进 swoole 后没有这层保护。

结论：

- 不要在同一个进程里静态链接第二份 OpenSSL。
- 如果必须静态链接，至少隐藏符号并绑定自身，且每次 OpenSSL 安全更新都要重编该扩展；仍然不推荐。
- wheel 只能替换动态库，改不了静态编进去的代码。

## 6. 本次修复的构建问题

### 6.1 旧依赖目录导致 `wheel.map` 重复

`wheelize_tree` 扫描 `deps/*/lib/*.so.*`。依赖版本更新后旧目录不会自动删除，新旧两份同名 SONAME 都会进入映射，消费方可能仍指向旧库。

修复：

- 构建时按 `dep_dir_of <name>` 清理同类旧版本目录；
- `wheelize_tree` 生成映射后检查重复 SONAME，发现重复直接报错。

### 6.2 外层判定重编，内层 stamp 跳过

外层 `dep_sig` / `ext_sig` 能看到依赖变化，但配方内部的 `SIG` 只看自身版本。外层决定重编后，配方可能被自己的 `stamp_ok` 挡住并 `exit 0`，随后外层把新签名写进 state，等于把旧产物标记成新签名。

修复：

- 外层决定构建时，用 `FORCE=1` 调用依赖配方和扩展配方，绕过内层 stamp；
- `dep_sig` 的 `needs` / `optional` 部分加入依赖的 state；
- 有依赖漂移的配方内层 `SIG` 补上依赖版本：
  - `deps/curl.sh`
  - `deps/libzip.sh`
  - `deps/libxml2.sh`
  - `ext/swoole.sh`
  - `ext/xdebug.sh`

## 7. 验证方法

```bash
# 当前扩展是否静态链接了 OpenSSL
readelf -d swoole.so | grep NEEDED
nm -D swoole.so | grep SSL_CTX_new
readelf -V swoole.so | grep OPENSSL

# 实际绑定了哪一份
LD_DEBUG=bindings ./myphp -d extension=swoole -r '' 2>&1 | grep SSL_CTX_new

# 运行时依赖
ldd bin/php
ldd lib/php/extensions/*/swoole.so

# wheel 映射是否有重复
cut -d'|' -f1,2 lib/phprt/wheel.map | sort | uniq -d

# 升级日志（phprt update 的记录）
cat .phprt/logs/upgrade.log

# 整树校验
phprt verify
```

## 8. 当前结论

1. wheel 命名用运行时名（rt + SONAME ABI 版本号），不用内容 hash；同系列
   小版本升级消费方零改写、零重编。
2. 一棵 PHP 树 / 一个进程只允许一个 OpenSSL 版本。
3. 依赖版本互不绑定；更新一个组件只重编依赖它的组件。
4. 扩展应动态链接 PHP 已加载的同一份 OpenSSL，不静态内嵌第二份。
5. 需要不同 OpenSSL 线路时，使用不同的 `.phprt/php-*` 树和独立进程。

## 9. 同系列小版本升级：phprt update

`phprt update <依赖> <版本>`（如 `phprt update openssl 3.5.8`）只升级一个
依赖库的同系列小版本：

1. 校验：只允许同系列（major.minor 相同）且更高版本，拒绝降级；
2. 加构建锁，快照全树 ELF 的 SONAME / NEEDED / RPATH；
3. 备份 bundle.conf、旧 deps 目录、lib/phprt、wheel.map；
4. 改仓库 bundle.conf 的版本变量和 URL（项目副本由 `bundle_for` 同步）；
5. 下载新源码包到 downloads/（旧包保留），`FORCE=1` 只重编该依赖；
6. 比对新旧 SONAME 集合，不一致当跨系列拒绝；
7. 移走旧 deps 目录，`wheelize_tree`；
8. 断言全树 ELF 链接关系快照零差异、wheel 完整性校验、`myphp` 功能验证
   （openssl 额外查 flat 里 libcrypto 的版本串；注意 `OPENSSL_VERSION_TEXT`
   是 PHP 编译期字符串，不重编就不会变，不能当验证依据）；
9. 修正其它依赖 `.la`/`.pc` 里指向旧依赖目录的路径（libtool 链接时会读
   `.la` 的 `dependency_libs`，不改掉以后重链接会指向已移走的旧目录）；
10. 写升级日志 `.phprt/logs/upgrade.log`（START / BUILD / WHEEL / VERIFY /
   COMMIT，失败写 ROLLBACK），清理备份。

任何一步失败都回滚：bundle.conf、旧 deps 目录、lib/phprt、wheel.map 全部
还原。

注意：升级后 PHP 构建树里的旧 `.dep`/`.la` 引用不会立刻清理（运行时不受影
响，wheel 已经是新库）。下次 `phprt install --recompile` 真要重编 PHP 时，`php.sh` 的
`FLAGS_SIG` 含 CPPFLAGS/LDFLAGS（依赖路径在里面），路径一变就会
`make distclean` 后全新构建，不会拿旧 `.dep` 碰壁。

限制：

- 只保证 ABI 兼容的同系列升级。消费者是旧头文件编的，不会用到新 API；
  禁止降级（wheel.map 记版本，verify 可查）。
- 同系列内换编译选项导致 ABI 变化的情况，靠 SONAME 集合比对 + sha256 +
  功能验证兜底，不能 100% 识别。
- 跨系列升级（如 3.5 → 3.6、OpenSSL 4.x）会被拒绝，走 `phprt install --recompile` 的
  正常重编路径。
