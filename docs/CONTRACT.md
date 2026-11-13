# recipe 契约

recipe 是 `recipes/**.star`，每个文件对应一个目标。`defs.star` 提供 `dep` / `ext` /
`php` / `feature` / `sub` 构造函数；校验全部在 Rust 侧。

一个 recipe 可以写成两种形式：

```python
# 静态 dict
TARGET = dep(...)

# 依赖注入：Rust 先解析版本线，再带着 ctx 调用
def target(ctx):
    ...
    return ext(...)

TARGET = target
```

`ctx` 由 Rust 注入：

```python
{
    "php_series": "8.5",
    "php_version": "8.5.12",
    "bindings": {"openssl": "3.5", "swoole": "6.2"},
    "deps": {
        "openssl": {"line": "3.5", "version": "3.5.9", "url": "...", "sha256": "..."},
        "libcurl": {"line": "8.23", "version": "8.23.0", "url": "...", "sha256": "..."},
    },
    "self": {"line": "6.2", "version": "6.2.5", "url": "...", "sha256": "..."},
    "is_builtin": False,
}
```

- `self` 是当前 recipe 同名组件在 catalog 里的记录；没有同名组件时为 `None`。
- `is_builtin` 由当前 PHP 系列的 `builtin_exts` 推导；dep/php 恒为 `False`。
- 版本号、下载地址、sha256 都从 ctx 取，recipe 不写死。
- 内置扩展用 `ctx["php_version"]` 作为自己的版本，url/sha256 留空；
  非内置扩展用 `self` 的版本和下载地址。

## 1. 版本关系

PHP 与依赖库版本的对应关系属于**扩展**，不属于 dep。dep 只描述库本身。

### ext 自己的版本线：`LINES`

有独立版本线的扩展（目前是 swoole、xdebug）声明：

```python
LINES = {
    "6.2": {
        "php": ["8.4", "8.5", "8.6"],               # 这条线支持哪些 PHP 系列
        "openssl": ["1.1.1", "3.0", "3.5", "4.0"],  # 依赖允许的线（可选）
    },
}
```

- `php` 必填且非空。
- 除了 `php`，其他 key 都当作依赖约束：依赖名 → 允许的版本线。
- 如果声明了 `LINES`，线集合必须和 `catalog` 里同名扩展的线完全一致。

### builtin 扩展的依赖线：`PHP_LINES`

`ext:openssl` 没有自己的版本线（版本跟 PHP 走），用 `PHP_LINES` 声明
“这个 PHP 的 openssl 扩展允许哪些 OpenSSL 库线”：

```python
PHP_LINES = {
    "7.4": ["1.1.1"],
    "8.0": ["1.1.1"],
    "8.2": ["1.1.1", "3.0", "3.5"],
    "8.5": ["1.1.1", "3.0", "3.5", "4.0"],
    "8.6": ["1.1.1", "3.0", "3.5", "4.0"],
}
```

- key 必须是 catalog 里的 PHP 系列，且每个系列都要有入口。
- value 里的线必须存在于 catalog 的同名组件（`ext:openssl` → `openssl`）。
- 只在选中该扩展时参与约束。

### dep 不写 PHP 关系

`dep` recipe 禁止声明 `LINES` / `PHP_LINES`。OpenSSL、libcurl、nghttp2
与 PHP 没有直接关系；哪个 PHP 能用哪条 OpenSSL 线，由消费它的扩展决定。

`target(ctx)` 只负责构建参数，不判断能不能配。

## 2. catalog

`recipes/catalog.toml` 只存“组件 + 线 → 当前最新补丁 + 下载地址”：

```toml
[swoole]
"6.2" = { version = "6.2.5", url = "https://pecl.php.net/get/swoole-6.2.5.tgz", sha256 = "..." }

# openssl.org 会 301 到 GitHub Releases，这里写跳转后的最终地址
[openssl]
"3.5" = { version = "3.5.9", url = "https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz", sha256 = "..." }
```

- `[php]` 必须有，key 是 PHP 系列（7.4 / 8.0 / 8.2 / 8.3 / 8.4 / 8.5 / 8.6）。
- 每条线必须有 `version` 和 `url`。
- `url` 写上游最终地址；跳转链路用注释记录，判定规则见 [ADD-RECIPE.md](ADD-RECIPE.md)。
- 不写 PHP 兼容性、不写构建参数。
- 补丁更新只改这里。

## 3. bundle

`recipes/bundles/php<系列>.toml` 写默认线和默认扩展：

```toml
php = "8.5"
openssl = "3.5"
swoole = "6.2"
xdebug = "3.5"

default_exts = "openssl curl=sub:curl[ssl]"
```

- `php` 必须和文件名一致。
- 版本线的值只能是 catalog 里存在的线；没有写 `version:` 的扩展使用这里的默认线。
- `default_exts` 是**空格分隔的字符串**，使用 `--ext` 的 spec 语法；
  不允许 `version:`；缺省为空。
- `default_exts` 里的名字必须是当前 PHP 系列 `builtin_exts` 的子集，并且有 ext recipe。
- 有依赖的内置扩展必须写在同一个字符串里（如 `"libxml xml"`）；
  不做依赖展开或校验，漏写时 PHP configure 会报错。

## 4. dep

```python
def target(ctx):
    self = ctx["self"]
    return dep(
        name="openssl",
        version=self["version"],
        url=self["url"],
        sha256=self["sha256"],
        needs=[],
        features={
            "ssl": feature(
                needs=["openssl"],
                configure=["--with-openssl={dep:openssl}"],
                disable=["--without-ssl"],
            ),
        },
        configure=["./Configure", "--prefix={prefix}", "shared"],
        make=["make", "make install_sw"],
        artifacts=["lib/libssl.so.3"],
        desc="OpenSSL 库",
    )

TARGET = target
```

字段与以前一致；新增/变化：

- `version` / `url` / `sha256` 由 ctx 注入。
- 安装前缀仍是 `.phprt/php-<php 版本>/deps/<name>-<version>`。
- 禁止 `exts` 和 `tools`。
- `features` 由 ext 的 `sub:` 间接启用；没启用的 feature 会应用它的 `disable`
  参数（用来关掉上游的自动探测），没有 `disable` 就不传。

## 5. ext

```python
def target(ctx):
    self = ctx["self"]
    return ext(
        name="swoole",
        modules=["swoole"],
        link=["shared"],            # 契约允许的链接方式
        version=ctx["php_version"] if ctx["is_builtin"] else self["version"],
        url="" if ctx["is_builtin"] else self["url"],
        sha256="" if ctx["is_builtin"] else self["sha256"],
        needs=["libcurl", "openssl"],
        tools=["autoconf"],
        subs={"curl": sub(dep="libcurl", configure=["--enable-swoole-curl"])},
        configure=["--enable-swoole", "--with-openssl-dir={dep:openssl}"],
        ini="27-swoole.ini",
    )

TARGET = target
```

规则：

- ext recipe 不写 `origin`。是否内置由 `php<系列>.star` 的 `builtin_exts` 推导；
  内置 → 默认 `static`，非内置 → 默认 `shared`，默认值必须在 `link` 里。
- `link` 非空，值只能是 `static` / `shared`。
- `link` 含 `shared` 时必须有 `ini`；内置还要 `shared_from`，
  非内置还要 `url` + `version`。
- `tools` 是构建期工具依赖：参与构建顺序和 `PATH`，不参与 include/lib；
  每个名字必须是 `dep/*.star` 里的目标。
- `configure` 是正向参数：编进 PHP 时交给 PHP 的 configure，编成 shared 时
  交给扩展自己的 configure。
- 关闭参数不在 ext recipe 里，来自 php recipe 的 `builtin_disable`。
  同一个内置扩展只会出现 `--with-*` 或 `--without-*` 其中一组。
- `subs` 是“子配置名 → `sub(dep, configure)`”：
  - `dep` 必须出现在 `needs` 里；
  - 方括号里的 feature 必须在该 dep 的 `features` 里。
- 版本线通过 `LINES` / `PHP_LINES` 声明，不在 recipe 里写死。

## 6. php

```python
TARGET = php(
    series="8.5",
    builtin_exts=["openssl", "curl", "libxml", ...],
    builtin_disable={
        "openssl": "--without-openssl",
        "libxml": "--without-libxml",
        "dom": "--disable-dom",
    },
    configure=["--prefix={tree}", "--with-config-file-scan-dir={tree}/etc/conf.d"],
    make=["make", "make install"],
    artifacts=["bin/php"],
)
```

- `series` 必填，且必须和文件名 `php<系列>.star` 一致。
- `version` / `url` / `sha256` 从 catalog 的 `[php]` 注入。
- `builtin_exts`：该 PHP 版本的内置扩展名，和是否有 recipe 无关；非空、无重复。
- `builtin_disable`：内置扩展名 → 单个关闭参数；key 必须在 `builtin_exts` 里。
- 每个支持的系列一个 recipe；当前是 7.4 / 8.0 / 8.2 / 8.3 / 8.4 / 8.5 / 8.6。

## 7. 占位符

| 占位符 | 展开为 |
| --- | --- |
| `{tree}` | `.phprt/php-<php 版本>` |
| `{deps}` | `{tree}/deps` |
| `{build}` | 当前节点的解包/构建目录 |
| `{prefix}` | 当前节点的安装前缀 |
| `{dep:名字}` | `{tree}/deps/<名字>-<版本>` |

未知占位符在渲染阶段报错。

## 8. 校验清单

加载时检查：

1. `name` 非空；dep 的 version/url；php 的 series/version/url。
2. dep 不出现 `exts`。
3. ext 的 `link`、默认值、shared 必需字段；ext 不允许 `builtin_exts` / `builtin_disable`。
4. `needs` 指向的 `dep:*` 存在。
5. `tools` 指向的 `dep:*` 存在。
6. `features.*.needs` 指向的 `dep:*` 存在。
7. `subs.*.dep` 指向的 `dep:*` 存在，且在 `needs` 里。
8. php 的 `builtin_exts` 非空、无重复；`builtin_disable` 的 key 是其子集，
   值是单个 `--without-*` / `--disable-*`。
9. catalog 的 PHP 系列与 php recipe 集合一致。
10. catalog 里每个组件都有同名 recipe；扩展声明 `LINES` 时线集合必须一致。
11. `LINES` 的每条线都有非空 `php` 数组。
12. dep recipe 不声明 `LINES` / `PHP_LINES`。
13. `PHP_LINES` 覆盖所有 PHP 系列，引用的线都在 catalog 里。
