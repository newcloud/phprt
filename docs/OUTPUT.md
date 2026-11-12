# 输出

## 1. 结构

`plan` 的输出是 `Sim`：

```text
Sim { root, tree, order: [Node] }
```

| 字段 | 说明 |
| --- | --- |
| `root` | `--root` 的展示值 |
| `tree` | `.phprt/php-<php 版本>` |
| `order` | 拓扑序节点数组 |

`Node` 字段：

| 字段 | 说明 |
| --- | --- |
| `id` | `kind:name` |
| `kind` | `dep` / `ext` / `php` |
| `name` / `version` | 目标名和版本 |
| `origin` | ext 的 `builtin` / `pecl`，其他为空 |
| `mode` | ext 的 `static` / `shared`，其他为空 |
| `source` | 源码地址；shared builtin 显示 PHP 源码里的目录 |
| `sha256` | recipe 里的校验和 |
| `unpack` | 解压目录；ext 无独立解压目录时为空 |
| `prefix` | 安装前缀 |
| `needs` | 实际依赖的 `dep:*` / `php:*` |
| `modules` | ext 提供的模块 |
| `subs` | ext 选中的子配置：`[{name, dep, features}]` |
| `features` | 该 dep 实际启用的 feature（含历史累计） |
| `configure` | 展开后的 configure 参数 |
| `steps` | shared ext 的构建步骤 |
| `make` | dep/php 的 make 步骤 |
| `artifacts` | 预期产物 |
| `ini` / `ini_lines` | shared ext 的 ini 文件和内容 |

## 2. 路径规则

| 对象 | 值 |
| --- | --- |
| 安装树 | `.phprt/php-<php 版本>` |
| dep 前缀 | `{tree}/deps/<name>-<version>` |
| 构建目录 | `build/<目标名>/<源码目录名>`，php 用 `build/php/<源码目录名>` |
| shared builtin 源码 | `build/php/<PHP 源码目录名>/<shared_from>` |
| shared PECL 源码 | 下载地址最后一段，解压到构建目录 |
| ini | `{tree}/etc/conf.d/<ini>` |
| `.so` | `lib/php/extensions/*/<name>.so` |

源码目录名取 URL 最后一段并去掉 `.tar.gz` / `.tar.xz` / `.tar.bz2` / `.tgz` / `.zip`。

## 3. 文本格式

每个节点一段：

```text
[3/7] ext:curl 8.5.12
  source     build/php/php-8.5.12/ext/curl
  prefix     .phprt/php-8.5.12
  needs      dep:libcurl, php:8.5.12
  modules    curl
  sub        curl [ssl, nghttp2] -> dep:libcurl
  step       phpize
  step       ./configure --with-php-config=... --with-curl=...
  step       make
  step       make install
  outputs    lib/php/extensions/*/curl.so
  ini        etc/conf.d/20-curl.ini
             extension=curl.so
```

标签随节点变化：

- static ext：每个 configure 参数一行 `flag`。
- shared ext：每个构建步骤一行 `step`。
- dep / php：`configure` 每个参数一行，`make` 每个步骤一行。
- 有值才输出：`source`、`sha256`、`unpack`、`needs`、`modules`、`subs`、
  `features`、`outputs`、`ini`。

`build order` 里 ext 显示 `(origin mode)`。

## 4. JSON

`plan --json` 输出 `Sim` 的序列化，字段与上表一致。示例：

```json
{
  "root": ".",
  "tree": ".phprt/php-8.5.12",
  "order": [
    {
      "id": "ext:curl",
      "kind": "ext",
      "name": "curl",
      "version": "8.5.12",
      "origin": "builtin",
      "mode": "shared",
      "source": "build/php/php-8.5.12/ext/curl",
      "needs": ["dep:libcurl", "php:8.5.12"],
      "subs": [
        {"name": "curl", "dep": "libcurl", "features": ["ssl", "nghttp2"]}
      ],
      "features": [],
      "steps": ["phpize", "./configure ...", "make", "make install"],
      "ini": "20-curl.ini",
      "ini_lines": ["extension=curl.so"]
    }
  ]
}
```

`list` 和 `graph` 只有文本输出。

## 5. 状态文件输出

`plan` 只打印计划，不写文件。

`install` / `update` 打印同样的计划，然后追加：

```text
已写入 ./.phprt/php-8.5/state.json（模拟）
```

`ext add` / `ext remove` 不打印计划，只输出：

```text
已加入扩展 swoole（待安装）
已写入 ./.phprt/php-8.5/state.json（模拟）
```

文件格式见 [STATE.md](STATE.md)。
