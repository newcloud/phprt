# recipe 契约

recipe 是 `recipes/**.star`，每个文件顶层定义一个 `TARGET`。`defs.star` 里的
`dep` / `ext` / `php` / `feature` / `sub` 只负责拼 dict，校验全部在 Rust 侧。

## 1. 通用字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `kind` | `dep` / `ext` / `php` | 由构造函数写入 |
| `name` | string | 目标名 |
| `version` | string | 版本，PHP 目标的 name 就是版本 |
| `url` | string | 源码包地址 |
| `sha256` | string | 可选，目前不校验 |
| `needs` | list[string] | 指向 dep |
| `exts` | list[string] | 仅 php，指向 ext |
| `modules` | list[string] | ext 提供的 PHP 模块 |
| `desc` | string | 说明，不参与求解 |

## 2. dep

```python
TARGET = dep(
    name="libcurl", version="8.11.1", url="https://...",
    needs=[],
    features={"ssl": feature(needs=["openssl"],
                             configure=["--with-openssl={dep:openssl}"])},
    configure=["./configure", "--prefix={prefix}"],
    make=["make", "make install"],
    artifacts=["lib/libcurl.so.4"],
)
```

- 必须有 `version` 和 `url`。
- 不允许 `exts`。
- `features` 是由 ext 的 `sub:` 间接启用的子功能；`configure` 和 `needs`
  只在启用时追加。
- 安装前缀约定为 `.phprt/php-<php 版本>/deps/<name>-<version>`。

## 3. ext

```python
TARGET = ext(
    name="curl",
    origin="builtin",              # builtin | pecl
    modules=["curl"],
    link=["static", "shared"],     # 契约允许的链接方式
    version="8.5.12",
    needs=["libcurl"],
    subs={                          # 扩展自己的子配置
        "curl": sub(dep="libcurl"), # 方括号里的 feature 转给 libcurl
    },
    configure=["--with-curl={dep:libcurl}"],
    shared_from="ext/curl",        # builtin 的源码位置
    url="", sha256="",             # pecl 的源码包
    ini="20-curl.ini",
    ini_lines=[],
)
```

规则：

- `origin` 只能是 `builtin` 或 `pecl`。
- 默认链接方式由 `origin` 决定：`builtin` → `static`，`pecl` → `shared`；
  默认值必须出现在 `link` 里，否则 recipe 加载失败。
- `link` 非空，值只能是 `static` / `shared`。
- `link` 含 `shared` 时必须有 `ini`；`builtin` 还要 `shared_from`，
  `pecl` 还要 `url` + `version`。
- `subs` 是“子配置名 → `sub(dep, configure)`”的表：
  - `dep`：`--ext ...=sub:名字[a,b]` 里的 feature 转给哪个 dep，必须出现在 `needs` 里；
    空表示这个子配置不转发。
  - `configure`：选中这个子配置时追加到 ext 的编译参数，可空。
- 选了子配置但没有方括号 → 只应用 `configure`，不传 feature。
- 完全不选某个子配置 → 它的 `configure` 不生效，也不碰依赖。
- `ini_lines` 为空时，shared 的 ini 默认写 `extension=<name>.so`。
- `static` 不使用 `ini` / `ini_lines`。

## 4. php

```python
TARGET = php(
    version="8.5.12",
    url="https://www.php.net/distributions/php-8.5.12.tar.gz",
    exts=["openssl", "curl", "swoole", "xdebug"],
    configure=["--prefix={tree}", "--with-config-file-scan-dir={tree}/etc/conf.d"],
    make=["make -j8", "make install"],
    artifacts=["bin/php"],
)
```

- 必须有 `version` 和 `url`。
- 当前只支持一个 php recipe。
- `exts` 里的扩展参与求解；static 的进 PHP configure，shared 的排在 PHP 之后。

## 5. 校验清单

加载时逐项检查：

1. `name` 非空；dep 的 `version`/`url`；php 的 `version`/`url`。
2. dep 不出现 `exts`。
3. ext 的 `origin`、`link`、默认值、shared 必需字段、`subs` 的 `dep` 必须在 `needs` 里。
4. `needs` 指向的 `dep:*` 存在。
5. `features.*.needs` 指向的 `dep:*` 存在。
6. `exts` 指向的 `ext:*` 存在。
7. `subs.*.dep` 指向的 `dep:*` 存在。
8. 目标 id 不重复。

## 6. 占位符

| 占位符 | 展开为 |
| --- | --- |
| `{tree}` | `.phprt/php-<php 版本>` |
| `{deps}` | `{tree}/deps` |
| `{build}` | 当前节点的构建目录 |
| `{prefix}` | 当前节点的安装前缀 |
| `{dep:名字}` | `{tree}/deps/<名字>-<版本>` |

未知占位符在渲染阶段报错。
