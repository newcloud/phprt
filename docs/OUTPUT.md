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
| `name` / `version` | 目标名和版本（补丁来自 catalog） |
| `line` | 选中的版本线，如 `6.2`；builtin 扩展为空 |
| `origin` | ext 的 `builtin` / `pecl`，由 php recipe 的 `builtin_exts` 推导 |
| `mode` | ext 的 `static` / `shared`，其他为空 |
| `source` | 源码地址；shared builtin 显示 PHP 源码里的目录 |
| `sha256` | catalog 里的校验和 |
| `unpack` | 解压目录；shared builtin 显示拷贝出来的构建目录 |
| `prefix` | 安装前缀 |
| `needs` | 实际依赖的 `dep:*` / `php:*` |
| `tools` | 构建期工具依赖，如 `dep:autoconf` |
| `modules` | ext 提供的模块 |
| `deps` | ext 选中的依赖集成：`[{name, features}]` |
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
| 项目入口 | `<root>/myphp` |
| 当前版本 | `<root>/.phprt/current` → `php-<php 版本>` |
| 依赖库 | `{tree}/lib/phprt` |
| dep 前缀 | `{tree}/deps/<name>-<version>` |
| 源码缓存 | `<root>/.phprt/downloads/<URL 最后一段>` |
| 解压目录 | `build/<目标名>/<源码目录名>`，php 用 `build/php/<源码目录名>` |
| shared builtin 源码 | `build/php/<PHP 源码目录名>/<shared_from>` |
| shared builtin 构建目录 | `build/<扩展名>/php-<PHP 版本>` |
| shared PECL 源码 | 下载地址最后一段，解压到构建目录 |
| ini | `{tree}/etc/conf.d/<ini>` |
| `.so` | `lib/php/extensions/*/<name>.so` |

源码目录名取 URL 最后一段（去掉查询串）并去掉 `.tar.gz` / `.tgz` / `.tar.xz` / `.txz` / `.tar.bz2` / `.zip`。

## 3. 文本格式

每个节点一段：

```text
[3/7] ext:curl 8.5.12
  source     build/php/php-8.5.12/ext/curl
  prefix     .phprt/php-8.5.12
  needs      dep:libcurl, php:8.5.12
  tools      dep:autoconf
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

- static ext：每个 `configure` 正向参数一行 `flag`。
- ext 选中的依赖集成输出一行 `dep        libcurl [ssl]`；选中的 feature 输出在
  `features` 行（与 dep 的 feature 共用这个字段）。
- php：`configure` 里每个内置扩展只出现正向参数或 php recipe
  `builtin_disable` 的关闭参数其中一组，不会同时出现 `--without-x` 和 `--with-x`。
- shared ext：每个构建步骤一行 `step`。
- dep / php：`configure` 每个参数一行，`make` 每个步骤一行。
- 有值才输出：`source`、`sha256`、`unpack`、`needs`、`tools`、`modules`、`deps`、
  `features`、`outputs`、`ini`。
- 首次项目会把 bundle 的 `default_exts` 算进方案；`plan` 输出里能直接看到这些节点。
- dep 的 `configure` 包含未启用 feature 的 `disable` 参数（如 `--without-ssl`）。

`build order` 里 ext 显示 `(origin mode, line X)`；php 显示补丁版本，dep 显示线对应的补丁。

需要版本线的节点还会多一行：

```text
  line       6.2
```

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
      "line": "",
      "origin": "builtin",
      "mode": "shared",
      "source": "build/php/php-8.5.12/ext/curl",
      "needs": ["dep:libcurl", "php:8.5.12"],
      "deps": [
        {"name": "libcurl", "features": ["ssl", "nghttp2"]}
      ],
      "features": [],
      "steps": ["phpize", "./configure ...", "make", "make install"],
      "ini": "20-curl.ini",
      "ini_lines": ["extension=curl.so"]
    }
  ]
}
```

`list` 和 `graph` 只有文本输出。`list` 第一行会打印当前 PHP 系列：

```text
series 8.5（PHP 8.5.12）
```

`ext list` 的列：

```text
name         origin    link            version    status
curl         builtin   static,shared   8.5.12     installed
swoole       pecl      shared          6.2.5      pending
xdebug       pecl      shared          3.5.3      available
```

版本列是当前系列下 recipe 解析出的版本；内置扩展跟随 PHP 补丁。
没有 state 时，bundle `default_exts` 里的扩展显示为 `pending`，其他 recipe 显示 `available`。

## 5. 下载输出

`install` / `update` 在计划之后输出下载过程；`--downloads` 只输出到下载结束。
所有下载日志走 stderr，stdout 仍然只有计划和结果行。

```text
源码缓存: .phprt/downloads
并发: 10 个文件（每站点 2），单文件分片 4（≥8 MB 才启用）
  [php] 8.5.12                     [██████████░░░░]  72%  10.1 MB / 13.9 MB  3.4 MB/s
  [deps] openssl 3.5.9             ⠹ 1.2 MB
  [ext] swoole 6.2.5               ⠸ 等待
  [php] 8.5.12                     ✓ 完成（13.9 MB）
下载完成: 1 个，已存在 0 个
```

- 进度框只在终端里启用；其他情况逐行输出。
- 缓存命中显示 `✓ 已存在（大小）`。
- 单个文件失败显示 `✗ 失败: 原因`，末尾再汇总。
- 失败汇总见 [DOWNLOAD.md](DOWNLOAD.md)。

## 6. 解压输出

下载之后按拓扑序逐个解压，和下载共用同一套进度框；非终端或 `--ui plain` 时逐行输出：

```text
解压源码（按构建顺序，3 个）
[1/3] [php] 8.5.12 解压 php-8.5.12.tar.xz → build/php/php-8.5.12
[php] 8.5.12 ✓ 解压完成
[deps] libcurl 8.23.0 ✓ 已解压
解压完成: 1 个，已存在 1 个
```

- 已存在的解压目录显示 `✓ 已解压`，不再打印进度行。
- 解压失败显示 `error: 解压 <文件>: 原因`，退出码 1，不写 state。
- 解析细节见 [EXTRACT.md](EXTRACT.md)。

## 7. 编译输出

解压之后按拓扑序编译，和下载/解压共用进度框；`--ui plain` 时逐行输出：

```text
编译并安装（按构建顺序，8 个）
[deps] openssl 3.5.9 ✓ 已构建
[3/8] [php] 8.5.12 configure
[3/8] [php] 8.5.12 make
[3/8] [php] 8.5.12 install
[php] 8.5.12 ✓ 完成
[ext] swoole 6.2.5 ✓ 已构建
编译完成: 2 个，已构建跳过 6 个
```

- 已完成的节点显示 `✓ 已构建`。
- 失败显示 `✗ 失败`，末尾打印日志路径和末尾 40 行，退出码 1，不写 state。
- 细节见 [BUILD.md](BUILD.md)。

## 8. 状态文件输出

`plan` 只打印计划，不写文件。

`install` / `update` 打印同样的计划、下载、解压和编译输出，然后追加：

```text
已写入 ./.phprt/php-8.5/state.json
```

`--downloads` 不写状态，只追加：

```text
源码已下载到 ./.phprt/downloads（未解压）
```

`ext add` / `ext remove` 不打印计划，只输出：

```text
已加入扩展 swoole（待安装）
已写入 ./.phprt/php-8.5/state.json
```

文件格式见 [STATE.md](STATE.md)。
