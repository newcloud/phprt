# 输出

## 1. 结构

`plan` 的输出是 `Sim`：

```text
Sim { root, tree, targets, toolchain, tools, order: [Node] }
```

| 字段 | 说明 |
| --- | --- |
| `root` | `--root` 的展示值 |
| `tree` | `.phprt/<产品>-<产品版本>` |
| `targets` | 本次构建目标：target 名 → 配置名 → 值；没有 targets 的产品为空对象 |
| `toolchain` | 实际编译器档位；产品没有 `toolchain` 声明时为 `null` |
| `tools` | 本次用到的构建工具（host + built）以及解析失败项 |
| `order` | 拓扑序节点数组；需要构建的工具节点排在最前面 |

`Node` 字段：

| 字段 | 说明 |
| --- | --- |
| `id` | 产品 `product:<名字>@<版本>`；工具 `tool:<名字>@<版本>`；组件 / 依赖 `component:<名字>` / `dep:<名字>` |
| `kind` | `dep` / `component` / `product` / `tool` |
| `name` / `version` | 目标名和版本（补丁默认来自 catalog；`--lock` 时用 state 指定、源码仍来自 catalog） |
| `line` | 选中的版本线，如 `6.2`；bundled 组件为空 |
| `source` | 组件的来源 `bundled` / `external`；产品 / 依赖为空 |
| `mode` | 组件挂载方式，如 `static` / `shared` / `dynamic`；其他为空 |
| `strategy` | 挂载策略 `fold` / `standalone`；其他为空 |
| `url` | 源码地址；bundled 组件为空 |
| `source_path` | bundled 组件在产品源码树里的相对路径 |
| `sha256` | catalog 里的校验和 |
| `unpack` | 解压目录；bundled standalone 组件显示拷贝出来的构建目录 |
| `prefix` | 安装前缀 |
| `needs` | 实际依赖的 `dep:*` / 产品节点 |
| `tools` | 本节点解析到的构建工具，如 `autoconf 2.72 (store)`、`make 4.4.1 (host)` |
| `for` | 工具节点专用：哪些节点需要这一版，按名字去重，如 `["xdebug"]`；其他节点为空 |
| `for_all` | 工具节点专用：`for` 覆盖本次全部非工具节点时为 `true`，文本显示 `all nodes` |
| `modules` | 组件提供的模块 |
| `deps` | 组件选中的依赖集成：`[{name, features}]` |
| `features` | 该组件 / 依赖实际启用的 feature（含历史累计） |
| `configure` | 展开后的 configure 参数 |
| `steps` | standalone 组件的构建步骤 |
| `make` | dep / product 的 make 步骤 |
| `artifacts` | 预期产物 |
| `ini` / `ini_lines` | PHP shared 组件的 ini 文件和内容 |

## 2. 路径规则

| 对象 | 值 |
| --- | --- |
| 安装树 | `.phprt/<产品>-<产品版本>` |
| 项目入口（PHP） | 选中 `cli` target 时写 `<root>/myphp-<系列>`；只有一个 PHP 系列时额外写 `<root>/myphp` |
| 依赖库 | `{tree}/<产品 profile 的 runtime.lib_dir>`；PHP / nginx 当前是 `{tree}/lib/phprt` |
| dep 前缀 | `{tree}/deps/<name>-<version>` |
| 源码缓存 | `<root>/.phprt/downloads/<URL 最后一段>` |
| 解压目录 | `build/<目标名>/<源码目录名>` |
| bundled standalone 组件源码 | `build/<产品>/<产品源码目录名>/<source_path>` |
| bundled standalone 组件构建目录 | `build/<组件名>/<产品>-<产品版本>` |
| external 组件源码 | 下载地址最后一段，解压到构建目录 |
| ini | `{tree}/etc/conf.d/<ini>` |
| `.so`（PHP） | `lib/php/extensions/*/<name>.so` |
| `.so`（nginx dynamic） | `modules/*.so` |

源码目录名取 URL 最后一段（去掉查询串）并去掉 `.tar.gz` / `.tgz` / `.tar.xz` / `.txz` /
`.tar.bz2` / `.zip`。

## 3. 文本格式

先打印头部、toolchain 段、tools 段和构建顺序，再逐节点输出：

```text
phprt plan
  root    /home/z/project
  tree    .phprt/php-7.4.33
  targets cli

toolchain
  cc        gcc 15.3.1 (>= 14)
  cxx       g++ 15.3.1 (>= 14)
  triple    x86_64-redhat-linux / x86_64
  cflags    -std=gnu17 -Wno-error=incompatible-pointer-types

tools
  ar  (host)
  autoconf 2.71 (store) -> build/tools/autoconf-2.71  for xdebug
  autoconf 2.72 (store) -> build/tools/autoconf-2.72  for zlib
  make 4.4.1 (host)
  patchelf 0.19.3 (store) -> build/tools/patchelf-0.19.3  for all nodes
  ...

build order
   1. tool:autoconf@2.71        for xdebug
   2. tool:autoconf@2.72        for zlib
   3. dep:openssl               3.5.9
   4. component:curl            (bundled static)
  ...
  25. product:php@8.5.12
  27. component:xdebug          (external shared, line 3.5)
```

每个节点一段：

```text
[4/27] component:curl 8.5.12
  from       ext/curl
  prefix     .phprt/php-8.5.12
  needs      dep:libcurl
  tools      autoconf 2.72 (store)
  modules    curl
  dep        libcurl [ssl]
  features   ssl
  flag       --with-curl=...
```

标签随节点变化：

- fold 组件：每个 `configure` 正向参数一行 `flag`，以及 profile 追加的参数。
- standalone 组件：每个构建步骤一行 `step`。
- product / dep：`configure` 每个参数一行，`make` 每个步骤一行。
- 组件选中的依赖集成输出一行 `dep        libcurl [ssl]`；选中的 feature 输出在
  `features` 行（与 dep 的 feature 共用这个字段）。
- product 的 `configure` 里每个 bundled 组件只出现正向参数或产品 recipe
  `bundled_disable` 的关闭参数其中一组，不会同时出现 `--without-x` 和 `--with-x`。
- 有值才输出：`source`、`from`、`sha256`、`unpack`、`needs`、`tools`、`modules`、`deps`、
  `features`、`outputs`、`ini`。
- `tools` 段在宿主工具名后不带版本时显示 `(host)`；built 工具显示 `(store) -> 前缀`；
  解析不出来的显示 `名字 (无法解析: 原因)`。`plan` 不因工具缺失失败。
- 首次项目会把 bundle 的 `default_components` 算进方案；`plan` 输出里能直接看到这些节点。
- dep 的 `configure` 包含未启用 feature 的 `disable` 参数（如 `--without-ssl`）。

`build order` 里组件显示 `(source mode, line X)`；产品不显示版本，dep 显示线对应的
补丁；工具的 id 已带 `@版本`，不再重复版本号。built 工具后面加 `for 消费者`；
消费者覆盖全部非工具节点时写 `for all nodes`。

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
  "targets": {
    "cli": {}
  },
  "toolchain": {
    "cc": "gcc",
    "gcc": "15.3.1",
    "cxx": "g++",
    "gxx": "15.3.1",
    "triple": "x86_64-redhat-linux",
    "arch": "x86_64",
    "gcc_band": ">= 9",
    "gxx_band": ">= 9",
    "cflags": "",
    "cxxflags": ""
  },
  "tools": [
    {"name": "make", "version": "4.4.1", "provider": "host", "prefix": ""},
    {"name": "autoconf", "version": "2.72", "provider": "store", "prefix": "build/tools/autoconf-2.72"}
  ],
  "order": [
    {
      "id": "component:curl",
      "kind": "component",
      "name": "curl",
      "version": "8.5.12",
      "line": "",
      "source": "bundled",
      "mode": "static",
      "strategy": "fold",
      "url": "",
      "source_path": "ext/curl",
      "needs": ["dep:libcurl"],
      "deps": [
        {"name": "libcurl", "features": ["ssl", "nghttp2"]}
      ],
      "features": [],
      "configure": ["--with-curl=..."],
      "ini": "",
      "ini_lines": []
    }
  ]
}
```

`list` 和 `graph` 只有文本输出。

`list` 不带参数：

```text
product        version      tree
php@8.5        8.5.12       .phprt/php-8.5.12
nginx@1.29     1.29.8       .phprt/nginx-1.29.8
```

`list <产品>@<系列>`：

```text
series 8.5（php 8.5.12）
name             source    modes              version    status
curl             bundled   static,shared      8.5.12     installed
swoole           external  shared             6.2.5      pending
xdebug           external  shared             3.5.3      available
```

版本列是当前系列下 recipe 解析出的版本；bundled 组件跟随产品补丁。
没有 state 时，bundle `default_components` 里的组件显示为 `pending`，其他 recipe 显示
`available`。

## 5. 下载输出

`install` / `update` 在计划之后输出下载过程；`--downloads` 只输出到下载结束。
所有下载日志走 stderr，stdout 仍然只有计划和结果行。

```text
源码缓存: .phprt/downloads
并发: 10 个文件（每站点 2），单文件分片 4（≥8 MB 才启用）
  [product] php 8.5.12             [██████████████████░░░░░░░░░░]  72%  10.04 MiB/13.94 MiB  3.41 MiB/s
  [deps] openssl 3.5.9             ⠹ 1.20 MiB
  [component] lua-nginx-module 0.10.31  ⠸ 等待
  [product] php 8.5.12             ✓ 完成（13.9 MB）
下载完成: 1 个，已存在 0 个
```

- 进度框只在终端里启用；其他情况逐行输出。
- 还没轮到的行显示 `⠸ 等待`，不转圈，免得看起来在跑。
- 长度未知的文件显示转圈和已下载字节数。
- 缓存命中显示 `✓ 已存在（大小）`。
- 单个文件失败显示 `✗ 失败: 原因`，末尾再汇总。
- 失败汇总见 [DOWNLOAD.md](DOWNLOAD.md)。

## 6. 解压输出

下载之后按拓扑序逐个解压，和下载共用同一套进度框；非终端或 `--ui plain` 时逐行输出：

```text
解压源码（按构建顺序，3 个）
[1/3] [product] php 8.5.12 解压 php-8.5.12.tar.xz → build/php/php-8.5.12
[product] php 8.5.12 ✓ 解压完成
[deps] libcurl 8.23.0 ✓ 已解压
解压完成: 1 个，已存在 1 个
```

- 已存在的解压目录显示 `✓ 已解压`，不再打印进度行。
- 解压失败显示 `error: 解压 <文件>: 原因`，退出码 1，不写 state。
- 解析细节见 [EXTRACT.md](EXTRACT.md)。

## 7. 编译输出

解压之后按拓扑序编译，和下载/解压共用进度框。终端里是这样：

```text
编译并安装（按构建顺序，9 个）
  [deps] openssl 3.5.9             ✓ 已构建
  [deps] libcurl 8.23.0            ✓ 已构建
  [product] php 8.5.12 make        ⠹ 1m 20s
  [component] swoole 6.2.5         ⠸ 等待
```

- 正在执行的节点显示转圈和这个节点的已用时间；时间在走，说明没卡住。
- 同一节点从 `configure` 换到 `make`、`install` 时只换动词，秒表不回零。
- 还没轮到的节点显示 `⠸ 等待`，不转圈，免得看起来在跑。
- 已完成的节点显示 `✓ 已构建`，从进度框移出，固定成一行。
- 标签跟节点类型走：`[deps]` / `[component]` / `[product]` / `[tool]`；工具节点的
  下载、解压、编译都显示 `[tool]`，不是 `[deps]`。
- `--ui plain` 时逐行输出：

```text
编译并安装（按构建顺序，9 个）
[deps] openssl 3.5.9 ✓ 已构建
[3/9] [product] php 8.5.12 configure
[3/9] [product] php 8.5.12 make
[product] php 8.5.12 ✓ 完成
[component] swoole 6.2.5 ✓ 已构建
编译完成: 2 个，已构建跳过 7 个
```

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

`add` / `remove` 不打印计划，只输出结果行。

`add`（该版本已装过，编译完成后）：

```text
已加入 xdebug（shared）并完成编译
已写入 ./.phprt/php-8.5/state.json
```

`add`（该版本还没装过，只记方案）：

```text
已加入 swoole（shared）；这个版本还没装过，先 phprt install php@8.5
已写入 ./.phprt/php-8.5/state.json
```

`add` 重复加、产物齐全：

```text
swoole 已在方案里（shared），产物齐全
```

`remove`（动态组件，先备份 ini）：

```text
已备份 ini: .phprt/backup/20261117-163035_swoole.ini
已移除 swoole（shared），删掉 2 个文件：
  lib/php/extensions/no-debug-non-zts-20250925/swoole.so
  etc/conf.d/27-swoole.ini
已移除组件 swoole
已写入 ./.phprt/php-8.5/state.json
```

`remove`（磁盘上已经没有可删的文件）：

```text
已移除 swoole（shared），磁盘上没有可删的文件
已移除组件 swoole
已写入 ./.phprt/php-8.5/state.json
```

`add` / `remove` 静态组件会先打印重编代价并确认，确认后重编，编译输出同 `install`。

文件格式见 [STATE.md](STATE.md)。

## 9. audit

```text
audit php@8.5

component:openssl
  source  build/php/php-8.5.12/ext/openssl
  --with-openssl                     configure
  --with-openssl-argon2              feature argon2
  --with-openssl-legacy-provider     ignored: 会改变默认 TLS 行为，需要单独验证
  feature argon2                   默认开  requires product 8.4/8.5/8.6，dep:openssl 3.5/4.0
```

- 每个组件 / 依赖一个段落；`source` 是源码目录，`note` 说明为什么拿不到清单。
- 有 `state.tools` 时先输出 `build tools` 段，列出上次构建用到的工具和 provider；
  工具没有上游选项，不出现在组件段落里。
- `--json` 输出 `{product, series, components[], tools[]}`；每个组件的 `options[]` 是
  `{option, state, detail?}`，`state` 为 `configure` / `feature` / `ignored` / `missing`。
- `features[]` 带 `default_on`（当前系列 bundle 是否默认开）和 `available`（requires 是否满足）。
- `audited=false` 的组件（产品 profile 的 `audit=none`）不要求处置选项。
