# 命令行

```bash
phprt [--recipes <目录>] [--root <目录>] <子命令> <产品>@<系列> [组件或目标] [选项]
```

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `--recipes` | `recipes` | recipe 目录；`catalog.toml`、`bundles/` 也在这里 |
| `--root` | `.` | 项目根目录；状态写在 `<root>/.phprt` 下 |

## 目标

所有涉及产品的命令都写 `产品@系列`：

```bash
phprt install php@8.5
phprt install nginx@1.29
phprt plan php@8.5
```

- `产品` 是 catalog 里的产品名：`php`、`nginx`。
- `系列` 是产品版本线，如 `8.5`、`1.29`；也接受该线上的补丁号（如 `php@8.5.12`）。
- 一条命令只处理一个产品。多产品就写多条命令：

```bash
phprt install php@8.5
phprt install nginx@1.29
phprt list
```

```text
error: install 只接受一个产品，例如 phprt install php@8.5；多个产品请分多条命令
```

没有默认产品、默认版本或当前上下文；`phprt add swoole`、`phprt install 8.5`、
`phprt --php 8.5 plan` 都直接报错。

## 子命令

| 命令 | 说明 | 写状态 |
| --- | --- | --- |
| `list` | 扫描 `.phprt/*/state.json`，列出已安装的产品 | 否 |
| `list <产品>@<系列> [--installed\|--pending]` | 列出该产品的组件及状态 | 否 |
| `graph <产品>@<系列>` | 按当前方案打印依赖边 | 否 |
| `plan <产品>@<系列> [目标] [--json]` | 求解并输出构建计划 | 否 |
| `install <产品>@<系列> [--downloads]` | 下载、解压、编译并安装；只接受一个产品 | 默认是，`--downloads` 否 |
| `update <产品>@<系列> [--downloads]` | 在该产品线上重新解析补丁并构建 | 默认是，`--downloads` 否 |
| `add <产品>@<系列> <组件>` | 把组件加入方案（待安装） | 是 |
| `remove <产品>@<系列> <组件>` | 从方案移除并重算 feature | 是 |
| `audit <产品>@<系列> [--json]` | 对照上游选项清单检查 recipe 的处置 | 否 |

`install` / `update` / `plan` 另外支持：

| 参数 | 说明 |
| --- | --- |
| `--component <SPEC>` | 本次临时选择组件；可重复。不写进状态 |
| `--dep <NAME=LINE>` | 绑定依赖库版本线；可重复 |
| `--disable-default-component <NAME>` | 禁用该产品系列的默认组件；可重复 |

持久化组件方案用 `add` / `remove`；`install --component` 只影响这一次构建。

## 组件

组件 spec 与 `add`、`--component` 共用：

```bash
phprt add php@8.5 swoole
phprt add php@8.5 'swoole=version:6.2,dep:libcurl[ssl]'
phprt remove php@8.5 swoole

phprt install php@8.5 --component swoole
phprt install nginx@1.29 --component lua-nginx-module
```

```text
spec := 名字
      | 名字=mode:挂载方式
      | 名字=dep:依赖
      | 名字=dep:依赖[feature,...]
      | 名字=feature:[名字,...]
      | 名字=version:线
      | 上述选项的任意组合，用逗号分隔
```

解析规则：

1. 一次只写一个组件；多个组件分多条命令。`a,b` 会报“一次只能指定一个组件”。
2. 按逗号切分，但 `[...]` 里的逗号不切；`mode:` / `dep:` / `feature:` / `version:`
   开头的片段并入前一个组件。
3. 名字后面的 `=` 和 `:` 等价：`curl=mode:shared` 和 `curl:mode:shared`。
4. `mode:` 的取值由产品 profile 定义：PHP 是 `static` / `shared`，nginx 是
   `builtin` / `dynamic`。不写时按组件的 source 取默认值
   （PHP：bundled→static、external→shared；nginx：bundled→builtin、external→dynamic）。
5. `dep:依赖` 打开这个依赖的集成：按 recipe 的 `dep_configure` 给组件加参数，不向依赖传 feature。
6. `dep:依赖[a,b]` 在打开集成的同时把 `a`、`b` 转给该依赖；feature 之间也可以用 `+`。
7. `feature:[a,b]` 打开组件自己的编译开关；开关可能在选中时额外引入一个依赖库
   （如 `swoole=feature:[cares]` 会编 `c-ares`）。必须带方括号；关闭全部 feature 写 `feature:[]`。
8. `feature:` 可以出现多次，取并集；跨列表重复的 feature 会去重。
9. 条件 feature（如 openssl 的 `argon2`）在产品系列或依赖线不满足时直接报错，并说明缺哪一条。
10. 不写 `feature:` 时，首次选中的组件使用 bundle 的 `[component_presets]`；写了就覆盖 preset。
11. `version:线` 指定组件自己的版本线；依赖库用 `--dep`。
12. 同一个组件只能出现一次；同一个依赖、同一个 version 只能出现一次。

示例：

```bash
# swoole 走 bundle 的 6.2 线
phprt add php@8.5 swoole
phprt install php@8.5

# swoole 固定 6.0 线，并把 ssl/nghttp2 转给 libcurl
phprt install php@8.2 --component swoole=version:6.0,dep:libcurl[ssl,nghttp2]

# openssl 打开 argon2（需要 PHP 8.4+ 且 openssl 3.5/4.0）
phprt install php@8.5 --component 'openssl=feature:[argon2]'

# nginx 动态模块
phprt add nginx@1.29 lua-nginx-module
phprt install nginx@1.29
```

## 构建方案

```text
无 state：
    方案 = bundle 的 [default_components] - --disable-default-component + --component
有 state：
    方案 = state.components - --disable-default-component + --component
```

- `[default_components]` 在 `recipes/bundles/<产品>/<系列>.toml`：`mode` / `features` / `deps`；
  可用 `ref = "component_presets.<同名组件>"` 引用 `[component_presets]`。
- `[component_presets]` 只提供选中组件时的默认 feature，不会默认安装组件。
- `--disable-default-component openssl` 从本次方案扣除；首次 install 成功后落盘。
- 和 `--component` 同名同时出现 → 报错。
- `install` / `update` 默认把方案写进状态；`--downloads` 不写。
- `plan` / `graph` 只做本次预览，不写状态。
- `add` 持久化添加；`remove` 持久化移除。
- 全新项目上 `add` / `remove` 会先把当前系列的默认集 seed 进 state。

```bash
phprt install php@8.5 --disable-default-component openssl
phprt add php@8.5 xdebug
phprt remove php@8.5 xdebug
```

phprt 不展开组件之间的依赖：需要 libxml 的扩展（如 DOM/SimpleXML）必须把 `libxml`
一起写进方案，漏写时 PHP configure 会报错。

## 下载与解压

`install` / `update` 默认在打印计划之后、写状态之前完成：

```text
下载（缓存命中跳过）→ 解压（构建顺序）→ 编译 → 写 state
```

```bash
phprt install php@8.5
phprt install php@8.5 --component swoole
phprt install nginx@1.29 --ui plain
```

`--downloads` 只下载源码，后面的解压和写状态都不做：

```bash
phprt install php@8.5 --downloads
phprt update php@8.5 --downloads
phprt install nginx@1.29 --downloads --offline
```

| 参数 | 默认 | 说明 |
| --- | --- | --- |
| `--downloads` | 关 | 只下载，不解压、不编译、不写状态 |
| `--download-jobs N` | 10 | 文件级并发 |
| `--download-host-jobs N` | 2 | 同一站点并发 |
| `--connections N` | 4 | 单文件分片数 |
| `--timeout N` | 900 | 单个请求总超时（秒） |
| `--proxy URL` | 无 | 覆盖环境里的代理 |
| `--offline` | 关 | 只用缓存，不联网 |
| `--ui auto\|plain\|progress` | auto | 下载、解压和编译的进度显示方式 |

- 缓存目录是 `<root>/.phprt/downloads/`，所有产品共用；
  解压目录是 `<root>/build/<名字>/<补丁>/`。
- 下载失败、解压失败或编译失败都退出 1，并且不写状态。
- `--downloads` 结束时只打印源码缓存位置，不修改 state。
- GitHub 直连失败或 30 秒没下完会换镜像；`PHPRT_GITHUB_DIRECT_TIMEOUT` 可改秒数，
  镜像开关和列表在 `recipes/phprt.toml`。
- 下载细节见 [DOWNLOAD.md](DOWNLOAD.md)，解压细节见 [EXTRACT.md](EXTRACT.md)，
  编译细节见 [BUILD.md](BUILD.md)。

## 编译

解压之后按构建顺序编译，日志在 `<root>/build/logs/`：

```bash
phprt install php@8.5
./myphp-8.5 -v

phprt install php@8.5 --component swoole
phprt install nginx@1.29
```

- 每个节点按签名跳过；签名变化先 `make distclean`。
- 依赖库收进 `.phprt/<产品>-<版本>/lib/phprt`，用 `$ORIGIN` rpath。
- PHP 成功后写项目根 `myphp-<系列>`；项目里只有一个 PHP 系列时再写 `myphp`。
  nginx 不生成入口。
- 需要系统 gcc / g++ / make / binutils / pkg-config / patchelf / m4 / perl；
  autoconf 和缺的纯 Perl 模块由 phprt 自己准备。

## audit

`audit` 读已解压的上游源码，列出每个组件的上游 configure 选项，以及 recipe 的处置：

```bash
phprt audit php@8.5
phprt audit php@8.5 --json
phprt audit nginx@1.29
```

- 选项三态：`configure`（固定）、`feature 名字`（暴露）、`ignored: 理由`（不暴露）。
- 组件有「未处理」或 `ignored` 写了上游不存在的选项 → 退出码 1。
- 源码没解压 → 该组件算未审计，同样退出码 1，提示先安装一次。
- 选项来源由产品 profile 的 `audit` 决定；`none` 的产品（如 nginx）不要求盘点组件选项。
- 依赖库只给数量统计：库的选项大多由 recipe 固定，不要求全部做成 feature。
- 详见 [FEATURES.md](FEATURES.md)。

## 版本线

`--component` / `add` 的 `version:` 选的是**版本线**，不是补丁号：

```bash
phprt install php@8.5 --component swoole=version:6.2
phprt install php@8.5 --component swoole=mode:shared,version:6.0,dep:libcurl[ssl]
```

- `6.2` 会从 `recipes/catalog.toml` 查到当前最新补丁，如 `6.2.5`。
- 不能写 `6.2.5`；补丁由 catalog 决定。
- 不写 `version:` 时使用 bundle 默认线，如 php 8.5 的 swoole `6.2`。
- `version:` 只作用于有独立版本线的组件；`openssl=version:4.0` 和
  `curl=version:...` 都会报错：内置组件没有自己的版本线。

依赖库的线用 `--dep`，它不会把组件加入方案：

```bash
phprt install php@8.5 --component swoole --dep openssl=4.0
phprt install nginx@1.29 --dep openssl=3.5
```

`--dep` 只接受 `dep/*.star` 里的依赖库；给内置组件指库线时两个参数分开写：

```bash
phprt install php@8.5 --component openssl --dep openssl=4.0
```

## list

```bash
phprt list                       # 所有已安装的产品
phprt list php@8.5               # 该产品的组件
phprt list php@8.5 --installed
phprt list php@8.5 --pending
```

产品汇总：

```text
product        version      tree
php@8.5        8.5.12       .phprt/php-8.5.12
nginx@1.29     1.29.8       .phprt/nginx-1.29.8
```

组件状态：

| 状态 | 含义 |
| --- | --- |
| `installed` | 在方案里，且已经编译安装并通过启动校验 |
| `pending` | 在方案里，但还没安装 |
| `available` | recipe 存在，但不在方案里 |

```text
name             source    modes              version    status
curl             bundled   static,shared      8.5.12     installed
swoole           external  shared             6.2.5      pending
xdebug           external  shared             3.5.3      available
```

`--installed` 和 `--pending` 互斥；不带产品时两个参数都报错。

## 校验与退出

| 情况 | 错误示例 |
| --- | --- |
| 目标没写 `@` | `目标要写成 产品@系列，例如 php@8.5；实际是 8.5` |
| 一条命令多个产品 | `install 只接受一个产品，例如 phprt install php@8.5；多个产品请分多条命令` |
| 未知产品系列 | `catalog 里没有 php 8.9（可用：7.4, 8.0, ...）` |
| 未知组件 | `未知组件 component:nope` |
| 契约不支持的挂载方式 | `component:xdebug 契约不支持 mode:static（支持：shared）` |
| 组件没有这个依赖集成 | `component:curl 没有依赖集成 nope（可写：libcurl）；需要在 recipe 的 dep_configure 里声明` |
| 组件没有这个 feature | `component:openssl 没有 feature nope（可用：argon2）` |
| feature 写法不对 | `component:openssl 的 feature: 后面必须写方括号，例如 feature:[argon2]` |
| feature 的产品条件不满足 | `component:openssl 的 feature argon2 需要产品系列 8.4, 8.5, 8.6，当前 8.2` |
| feature 的依赖线条件不满足 | `component:openssl 的 feature argon2 需要 dep:openssl 3.5, 4.0，当前 3.0` |
| feature 需要的命令不在 PATH | `dep:jpeg 的 feature simd 需要命令 nasm（Fedora: sudo dnf install nasm）` |
| 内置组件写版本线 | `component:openssl 是内置组件，没有自己的版本线；依赖库 openssl 的版本请用 --dep openssl=<线>` |
| `--dep` 写了组件名 | `--dep swoole 不是依赖库` |
| 上游选项未处置 | `N 项没有通过 audit：...` |
| 依赖没有这个 feature | `dep:libcurl 契约没有 feature brotli` |
| 版本线不存在 | `「swoole」没有版本线 6.3（可用：4.8, 5.1, 6.0, 6.2）` |
| 组件没有版本表 | `「nope」没有可选版本（catalog 里没有这个组件）` |
| 版本线不支持当前产品 | `swoole 4.8 不支持产品 8.5（可用：6.2）` |
| 显式 pin 冲突 | `openssl 3.5 与当前产品/组件选择冲突（可用：1.1.1）` |
| 禁用非默认组件 | `--disable-default-component swoole 不是 php@8.5 的默认组件（默认：curl, libxml, openssl）` |
| 同时启用和禁用 | `--component 和 --disable-default-component 同时指定了 openssl` |
| default_components ref 写法不对 | `default_components.gd 的 ref 不合法：ref 必须是 "component_presets.<名字>"` |
| 状态文件是旧格式 | `state.json 是旧格式（version=2）...` |
| 移除不在方案里的组件 | `component:nope 不在构建方案里` |
| 离线且缓存里没有源码包 | `离线模式，缓存里没有这个文件` |
| 源码包下载失败 | `N 个源码包下载失败`（退出码 1，不写状态） |
| 解压时源码包不在缓存 | `源码包不在缓存: ...`（提示先 `--downloads`） |
| 不认识的压缩格式 | `不认识的压缩格式: x.tar.zst（支持 ...）` |
| 压缩包越界路径 | `压缩包里有越界路径: ../x` |
| 解压失败 | `解压 <文件> 失败: ...`（退出码 1，不写状态） |

除 Ctrl-C（130）外，任何错误都退出码 1。
