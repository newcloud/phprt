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

- 库名加 8 位内容 hash；
- 库自己的 SONAME 改成带 hash 的名字；
- `bin/php`、扩展和其他库的 `DT_NEEDED` 改到新名字；
- 所有 ELF 的 RPATH 改成 `$ORIGIN` 相对路径。

示例：

```text
libssl.so.3      → libssl-7ce83f8a.so.3
libcrypto.so.3   → libcrypto-f8719f94.so.3
libz.so.1        → libz-523f183f.so.1
```

目标只有三个：

1. 不加载系统上的同名动态库；
2. 整棵树可以改名或搬家；
3. 运行时不依赖 `LD_LIBRARY_PATH`。

内容 hash 不负责处理“多个版本必须共存”的问题，也不要求同一组依赖一起升级。

增量构建时 `wheel.map.prev` 保存上一轮映射，依赖重编后把消费方里的旧 hash 名改成新名字。

## 2. 依赖更新是否联动

版本上互不绑定；是否重编由依赖签名决定。

| 更新对象 | 其他组件版本要跟着变吗 | 当前签名策略下会重编 |
| --- | --- | --- |
| swoole | 不用 | 只 swoole |
| curl | 不用 | curl + PHP（curl 是内建扩展） |
| libzip | 不用 | libzip + PHP（zip 是内建扩展） |
| openssl | 不用 | openssl + curl + libzip + PHP + swoole |

openssl 那一行不是 wheel 要求的，而是 curl、libzip、swoole 的依赖签名里包含 openssl 目录和状态。

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
LD_DEBUG=libs → 只加载一份 libssl-<hash>.so.3
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
cut -d' ' -f1 lib/phprt/wheel.map | sort | uniq -d

# 整树校验
phprt verify
```

## 8. 当前结论

1. 保留内容 hash 的 wheel 命名。
2. 一棵 PHP 树 / 一个进程只允许一个 OpenSSL 版本。
3. 依赖版本互不绑定；更新一个组件只重编依赖它的组件。
4. 扩展应动态链接 PHP 已加载的同一份 OpenSSL，不静态内嵌第二份。
5. 需要不同 OpenSSL 线路时，使用不同的 `.phprt/php-*` 树和独立进程。
