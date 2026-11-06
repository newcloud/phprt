# Swoole 与 OpenSSL 的依赖关系

- 状态：记录当前实现、上游提交和结论
- 日期：2026-11-05
- 适用：phprt 的 swoole 6.x 扩展（PHP 8.5.12 实测）
- 相关：`docs/RUNTIME-LIB-VERSIONING.md`

## 1. 结论

swoole 依赖 OpenSSL，但要分三层看：

| 层面 | 当前状态 | 由什么决定 |
| --- | --- | --- |
| 编译期 | 强制依赖 | swoole 6.x 的 SSL 代码无条件编译，必须有 OpenSSL 头文件 |
| 链接期 | 不强制 | `swoole.so` 当前没有 `libssl` / `libcrypto` 的 `DT_NEEDED` |
| 运行期 | 必须有 libssl | SSL 符号由进程全局作用域里已加载的 libssl 提供 |

因此：

```text
ldd swoole.so / readelf -d swoole.so 看不到 libssl
≠ OpenSSL 没启用
≠ swoole 不需要 OpenSSL
```

它表示 swoole 没有把 libssl 记录成自己的运行时依赖，而是依赖 PHP 直接或间接加载的那一份。

## 2. swoole 6.x 把 OpenSSL 变成必需项

关键提交：

```text
dea57f44cc  2025-12-22
Change openssl from optional build parameter to required (#5965)
```

- commit：https://github.com/swoole/swoole-src/commit/dea57f44ccefdfae534e2b0d972a8263502cd1fa
- PR：https://github.com/swoole/swoole-src/pull/5965

这个提交做了三件事：

1. 删除 `PHP_ARG_ENABLE([openssl], ...)`，所以 `--enable-openssl` 从 6.2.4 起是无效参数。
2. 删除 `AC_DEFINE(SW_USE_OPENSSL, 1)` 和所有 `#ifdef SW_USE_OPENSSL`，SSL 代码变成无条件编译。
3. 删除显式的 `PHP_ADD_LIBRARY(ssl, 1, SWOOLE_SHARED_LIBADD)` 和 crypto 对应行。

`v6.2.4`（2026-10-25）包含该提交，当前 `master` 也还是同样的代码：

- https://github.com/swoole/swoole-src/blob/v6.2.4/config.m4#L1280
- https://github.com/swoole/swoole-src/blob/master/config.m4#L1285

当前源码树里 `SW_USE_OPENSSL` 出现次数为 0。

## 3. `SW_USE_OPENSSL` 曾经是做什么的

它是 swoole 自己的编译期特性开关，不是运行时检测，也不读 PHP 的 openssl 扩展状态。

旧版 `config.m4`：

```m4
if test "$PHP_OPENSSL" != "no" || test "$PHP_OPENSSL_DIR" != "no"; then
    ...
    AC_DEFINE(SW_USE_OPENSSL, 1, [enable openssl support])
    ...
fi
```

它控制的内容包括：

- `SWOOLE_SSL` 等 PHP 常量是否注册
- `include/swoole_ssl.h` 是否包含 OpenSSL 头文件和 SSL 类
- `onSSLReady`、`php_swoole_socket_set_ssl()` 等字段和函数是否编译
- 没有 SSL 时，客户端/服务端传 `ssl=true` 是否直接抛
  `you must configure with --enable-openssl ...`
- `php --ri swoole` 里是否显示 `openssl` / `dtls`

它只决定“SSL 代码在不在”，不决定 `DT_NEEDED`，也不检查 PHP。

## 4. 为什么 `swoole.so` 没有 libssl 的 `DT_NEEDED`

当前 `swoole.sh` 传的是：

```bash
--with-openssl-dir="$D_OPENSSL"
```

于是 swoole 的 `config.m4` 走这个分支：

```m4
if test "$PHP_OPENSSL_DIR" != "no"; then
    PHP_ADD_INCLUDE("${PHP_OPENSSL_DIR}/include")
    PHP_ADD_LIBRARY_WITH_PATH(ssl, "${PHP_OPENSSL_DIR}/${PHP_LIBDIR}")
    PHP_ADD_LIBRARY_WITH_PATH(crypto, "${PHP_OPENSSL_DIR}/${PHP_LIBDIR}")
else
    PKG_CHECK_MODULES([SSL], [libssl])
    PHP_EVAL_LIBLINE($SSL_LIBS, SWOOLE_SHARED_LIBADD)
    ...
fi
```

PHP 的宏定义（`build/php.m4`）：

```m4
AC_DEFUN([PHP_ADD_LIBRARY_WITH_PATH],[
ifelse($3,,[
  if test -n "$2"; then
    PHP_ADD_LIBPATH($2)
  fi
  PHP_ADD_LIBRARY($1)        # 只有两个参数：-lssl 进全局 LIBS
],[
  $3="-l$1 [$]$3"            # 有第三个参数：进指定变量
  ...
])
```

这里只传了两个参数，所以：

- `-L.../openssl/lib` 进 `LDFLAGS`
- `-lssl -lcrypto` 进全局 `LIBS`

而 phpize 生成共享扩展的链接行，结尾只拼 `SWOOLE_SHARED_LIBADD`：

```make
SWOOLE_SHARED_LIBADD = -lz -lpthread

$(LIBTOOL) ... $(shared_objects_swoole) $(SWOOLE_SHARED_LIBADD)
```

`config.log` 里能看到库确实被 configure 找到了，只是没进链接行：

```text
LIBS='-lstdc++ -lcrypto -lssl -lpthread '
```

GNU ld 默认允许共享库保留未定义符号，所以链接不报错，符号留到运行时解析。

## 5. swoole 是否有意避免成为 OpenSSL 的独立链接方

从提交历史看，**不是有意的**。

改动前，`--with-openssl-dir` 分支除了 `PHP_ADD_LIBRARY_WITH_PATH`，还有：

```m4
PHP_ADD_LIBRARY(ssl, 1, SWOOLE_SHARED_LIBADD)
PHP_ADD_LIBRARY(crypto, 1, SWOOLE_SHARED_LIBADD)
```

`dea57f44cc` 把这两行删了，但同一个提交保留的 pkg-config 分支仍然：

```m4
PHP_EVAL_LIBLINE($SSL_LIBS, SWOOLE_SHARED_LIBADD)
PHP_EVAL_LIBLINE($CRYPTO_LIBS, SWOOLE_SHARED_LIBADD)
```

两个分支行为不一致，更像是“改成必需项”时的遗漏，而不是为了降低多版本共存风险而刻意让 swoole 不链接。

不过，“一个进程只允许一份 OpenSSL”这个约束本身是正确的，必须保留：

- PHP 扩展通过 `dlopen` + `RTLD_GLOBAL` 加载，符号查找先看全局作用域。
- 如果进程里同时出现两份不同版本的 libssl，先加载的那份会参与符号绑定。
- `SSL_CTX`、`X509`、`EVP_PKEY` 等对象跨两份库传递是未定义行为。
- 静态链接第二份 OpenSSL 风险更高：符号插入、全局状态分裂，安全补丁可能被绕过。

需要区分两件事：

| 做法 | 是否会产生第二份 OpenSSL |
| --- | --- |
| swoole 动态 NEEDED 同一个 libssl（同 SONAME/同文件） | 不会，加载器复用 |
| swoole 动态 NEEDED 另一个 SONAME（如 libssl.so.4） | 会 |
| swoole 静态链接 OpenSSL | 会，且风险最高 |
| swoole 自己 `dlopen` 另一份 libssl | 会 |

所以“让 swoole 显式链接”本身不等于“多版本共存”；只要链接的是同一个文件，进程里仍然只有一份。

## 6. 运行时符号从哪里来

当前树中：

```text
bin/php
  NEEDED libssl-rt3.so.3
  NEEDED libcrypto-rt3.so.3
  NEEDED libcurl-rt4.so.4
  NEEDED libzip-rt5.so.5

libcurl
  NEEDED libssl-rt3.so.3
  NEEDED libcrypto-rt3.so.3

libzip
  NEEDED libcrypto-rt3.so.3

swoole.so
  NEEDED libz-rt1.so.1
  没有 libssl / libcrypto
```

swoole 被 `dlopen` 后，`SSL_CTX_new` 等未定义符号按 ELF 规则解析：

```text
自身 NEEDED → 全局作用域
全局作用域 = 主程序 bin/php
           + bin/php 的 NEEDED 闭包
           + 之前以 RTLD_GLOBAL 加载的库
```

实测：

```text
binding file .../swoole.so to .../libssl-7ce83f8a.so.3:
normal symbol `SSL_CTX_new'
```

注意：`libzip` 只提供 `libcrypto`，不提供 `SSL_*`。如果 PHP 的 openssl 和 curl 的 openssl 都关掉，进程里就没有 libssl，swoole 第一次 TLS 操作才会失败。因为当前没有 `BIND_NOW`，加载 `swoole.so` 本身不会立刻失败。

## 7. 对 phprt 和 `update openssl` 的影响

`swoole.sh` 里的声明：

```bash
#  phprt: switch=ENABLE_SWOOLE needs=openssl,zlib
```

`needs=openssl` 的作用：

1. `ensure_needs swoole`：构建 swoole 前先建好 openssl 依赖；
2. `dep_env swoole`：注入 `CPPFLAGS`、`LDFLAGS`、`PKG_CONFIG_PATH`；
3. `ext_sig`：把 openssl 目录和 state 算进签名，OpenSSL 更新时 swoole 会重编。

它不生成 `DT_NEEDED`。`needs` 是 phprt 的构建元数据，`readelf` 看不到。

更新 OpenSSL 时，预计重编：

```text
openssl → curl → libzip → PHP → swoole
```

约束：

- 一棵 PHP 树 / 一个 PHP 进程只允许一份 OpenSSL；
- 主版本必须一致，例如 PHP、curl、libzip、swoole 全部使用 `3.5.x`；
- 不要把 swoole 静态链接到另一份 OpenSSL；
- 不要让 swoole 单独 `dlopen` 另一份 libssl。

## 8. 可选的修复方式

当前行为能工作，但依赖是隐式的。需要显式时，有两种改法。

### 方案 A：走 pkg-config 分支

改 `ext/swoole.sh`：

- 去掉 `--with-openssl-dir="$D_OPENSSL"`
- 依赖 `dep_env` 设置的 `PKG_CONFIG_PATH`
- 需要 `pkg-config` 和 `deps/openssl-*/lib/pkgconfig/*.pc`

这样 `config.m4` 会走 `PHP_EVAL_LIBLINE($SSL_LIBS, SWOOLE_SHARED_LIBADD)`，`swoole.so` 出现：

```text
NEEDED libssl.so.3
NEEDED libcrypto.so.3
```

### 方案 B：保留参数，补链接行

configure 之后改生成的 `Makefile`：

```bash
sed -i 's|^SWOOLE_SHARED_LIBADD = |SWOOLE_SHARED_LIBADD = -lssl -lcrypto |' Makefile
```

这两个方案都只是让依赖显式化，不会引入第二份 OpenSSL。

## 9. 验证命令

```bash
TREE=.phprt/php-8.5.12
EXT="$TREE/lib/php/extensions/$(basename "$(ls -d "$TREE"/lib/php/extensions/*)")/swoole.so"

# 当前预期：只有 libz，没有 libssl / libcrypto
readelf -d "$EXT" | grep NEEDED

# 未定义符号：说明确实用了 OpenSSL
nm -D "$EXT" | grep ' U SSL_' | head

# 运行时绑定到哪一份 libssl
LD_DEBUG=bindings ./myphp -r '
Swoole\Coroutine\run(function () {
    $c = new Swoole\Coroutine\Http\Client("example.com", 443, true);
    $c->get("/");
});
' 2>&1 | grep SSL_CTX_new
```

如果改成显式链接，第一条命令应该能看到 `libssl-rt3.so.3` 和 `libcrypto-rt3.so.3`。

## 10. 参考链接

- 上游提交：https://github.com/swoole/swoole-src/commit/dea57f44ccefdfae534e2b0d972a8263502cd1fa
- 上游 PR：https://github.com/swoole/swoole-src/pull/5965
- v6.2.4 `config.m4`：https://github.com/swoole/swoole-src/blob/v6.2.4/config.m4#L1280
- 当前 `master` `config.m4`：https://github.com/swoole/swoole-src/blob/master/config.m4#L1285
- PHP 宏定义：https://github.com/php/php-src/blob/PHP-8.5/build/php.m4
- 本地对应文件：`/home/z/testphp/blog/.phprt/build/php-8.5.12/build/php.m4`
- wheel / 运行时库策略：`docs/RUNTIME-LIB-VERSIONING.md`
