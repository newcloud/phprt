# 命令行

```bash
phprt [--recipes <目录>] [--root <目录>] [--php <系列>] [--ext <SPEC>] [--dep <NAME=LINE>] [--disable-default-ext <NAME>] <子命令>
```

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `--recipes` | `recipes` | recipe 目录；`catalog.toml` 和 `bundles/` 也在这里 |
| `--root` | `.` | 项目根目录；状态写在 `<root>/.phprt` 下 |
| `--php` | 无 | PHP 系列，如 `8.5`；`install` 用位置参数，其他命令用这个 |
| `--ext` | 无 | 扩展选择，同时把点名的扩展加入本次方案 |
| `--dep` | 无 | 只绑定依赖库的版本线，如 `openssl=3.5` |
| `--disable-default-ext` | 无 | 禁用当前系列 bundle 里的默认扩展；可重复，只用于 install/update/plan |

## 子命令

| 命令 | 说明 | 写状态 |
| --- | --- | --- |
| `list` | 列出当前 PHP 系列下的 recipe | 否 |
| `graph` | 按当前方案打印依赖边 | 否 |
| `plan [目标] [--json]` | 求解并输出构建计划 | 否 |
| `audit [--json]` | 对照上游选项清单检查 recipe 的处置 | 否 |
| `install <PHP 版本> [--downloads]` | 下载、解压、编译并安装；必须指定 PHP | 默认是，`--downloads` 否 |
| `update [--downloads]` | 在已记录的线内重新解析补丁并构建 | 默认是，`--downloads` 否 |
| `ext list [--installed\|--pending]` | 列出扩展及版本 | 否 |
| `ext add <名字>` | 加入方案（待安装） | 是 |
| `ext remove <名字>` | 从方案移除并重算 feature | 是 |

## PHP 版本

`install` 必须带 PHP 版本：

```bash
phprt install 8.5
phprt install 8.5.12
phprt install php:8.5.12
```

不带时直接报错：

```text
error: 请指定 PHP 版本，例如：phprt install 8.5（可用：7.4, 8.0, 8.2, 8.3, 8.4, 8.5, 8.6）
```

其他命令用 `--php`：

```bash
phprt --php 8.5 plan
phprt --php 7.4 ext add swoole
```

省略 `--php` 时：其他命令在 `.phprt/` 下只有一个状态文件时用它，多个或没有则报错；
`list` 不是恰好一个状态时使用 catalog 里最高的 PHP 系列。

## 下载与解压

`install` / `update` 默认在打印计划之后、写状态之前完成两件事：

```text
下载（缓存命中跳过）→ 解压（构建顺序）→ 写 state
```

```bash
phprt install 8.5
phprt install 8.5 --ext swoole
phprt install 8.5 --ext swoole --ui plain
```

`--downloads` 只下载源码，后面的解压和写状态都不做：

```bash
phprt install 8.5 --downloads
phprt update --downloads
phprt install 8.5 --ext swoole --downloads --offline
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

- 缓存目录是 `<root>/.phprt/downloads/`；解压目录是 `<root>/build/<组件>/<补丁>/`。
- 下载失败、解压失败或编译失败都退出 1，并且不写状态。
- `--downloads` 结束时只打印源码缓存位置，不修改 state。
- GitHub 直连失败或 30 秒没下完会换镜像；`PHPRT_GITHUB_DIRECT_TIMEOUT` 可改秒数，
  镜像开关和列表在 `recipes/phprt.toml`。
- 下载细节见 [DOWNLOAD.md](DOWNLOAD.md)，解压细节见 [EXTRACT.md](EXTRACT.md)，
  编译细节见 [BUILD.md](BUILD.md)。

## 编译

解压之后按构建顺序编译，日志在 `<root>/build/logs/`：

```bash
phprt install 8.5
./myphp -v
phprt install 8.5                        # 默认就带 openssl + curl(ssl)
phprt install 8.5 --ext swoole --ext xdebug
```

- 每个节点按签名跳过；签名变化先 `make distclean`。
- 依赖库收进 `.phprt/php-<版本>/lib/phprt`，用 `$ORIGIN` rpath。
- 成功后写项目根 `myphp`，`.phprt/current` 指向本次安装树。
- 需要系统 gcc / g++ / make / binutils / pkg-config / patchelf / m4 / perl；
  autoconf 和缺的纯 Perl 模块由 phprt 自己准备。

## 构建方案

```text
无 state：
    方案 = bundle 的 [default_exts] - --disable-default-ext + --ext
有 state：
    方案 = state.ext - --disable-default-ext + --ext
```

- `[default_exts]` 是 `recipes/bundles/php<系列>.toml` 里的表：`link` / `features` / `deps`。
- 8.5 默认 `openssl` + `curl`（curl 带 ssl）；其他系列暂时没有默认扩展。
- `--disable-default-ext openssl` 从本次方案扣除；首次 install 成功后落盘。
- 和 `--ext` 同名同时出现 → 报错。
- 已经不在方案里的默认扩展 → 幂等跳过。
- `install` / `update` 默认把方案写进状态；`--downloads` 不写。
- `plan` / `graph` 只做本次预览，不写状态；`plan` 支持 `--disable-default-ext`。
- `ext add` 持久化添加；`ext remove` 持久化移除。
- 全新项目上 `ext add/remove` 会先把当前系列的默认集 seed 进 state。

```bash
phprt install 8.5 --disable-default-ext openssl
phprt install 8.5 --disable-default-ext openssl --disable-default-ext curl
phprt plan --php 8.5 --disable-default-ext curl
```

有依赖的内置扩展必须一起启用/禁用，例如 `libxml` 和 `xml`；phprt 不做依赖展开，
漏写时 PHP configure 会报错。

## audit

`audit` 读已解压的上游源码，列出每个扩展/依赖的上游 configure 选项，以及 recipe 的处置：

```bash
phprt --php 8.5 audit
phprt --php 8.5 audit --json
```

- 扩展选项三态：`configure`（固定）、`feature 名字`（暴露）、`ignored: 理由`（不暴露）。
- 扩展有「未处理」或 `ignored` 写了上游不存在的选项 → 退出码 1。
- 源码没解压 → 该组件算未审计，同样退出码 1，提示先安装一次。
- 依赖库只给数量统计：库的选项大多由 recipe 固定，不要求全部做成 feature。
- 详见 [FEATURES.md](FEATURES.md)。

## 版本线

`--ext` 的 `version:` 选的是**版本线**，不是补丁号：

```bash
phprt install 8.5 --ext swoole=version:6.2
phprt install 8.5 --ext swoole=link:shared,version:6.0,dep:libcurl[ssl]
```

- `6.2` 会从 `recipes/catalog.toml` 查到当前最新补丁，如 `6.2.5`。
- 不能写 `6.2.5`；补丁由 catalog 决定。
- 写 `--ext swoole` 不写 `version:` 时，使用 bundle 默认线，如 php8.5 的 `6.2`。
- `--ext curl=version:...` 会报错：curl 是 builtin，没有独立版本线。

依赖库的线用 `--dep`，它不会把扩展加入方案：

```bash
phprt install 8.5 --ext swoole --dep openssl=version:4.0
phprt install 8.5 --ext swoole --dep openssl=4.0
```

`--ext openssl=version:4.0` 是语法糖：安装 ext:openssl，同时绑定 `openssl` 依赖线。

## ext list

```bash
phprt --php 8.5 ext list
phprt --php 8.5 ext list --installed
phprt --php 8.5 ext list --pending
```

状态取值：

| 状态 | 含义 |
| --- | --- |
| `installed` | 在方案里，且已经编译安装并通过启动校验 |
| `pending` | 在方案里，但还没安装 |
| `available` | recipe 存在，但不在方案里 |

```text
name         origin    link            version    status
curl         builtin   static,shared   8.5.12     installed
swoole       pecl      shared          6.2.5      pending
xdebug       pecl      shared          3.5.3      available
```

`--installed` 和 `--pending` 互斥。

## `--ext` 语法

```text
--ext <spec>[,<spec>...]        # 可以重复，每次一个 spec
spec := 名字
      | 名字=link:static|shared
      | 名字=dep:依赖
      | 名字=dep:依赖[feature,...]
      | 名字=feature:[名字,...]
      | 名字=version:线
      | 上述选项的任意组合，用逗号分隔
```

解析规则：

1. `--ext` 可以写多次，每次一个 spec；点名的扩展会加入本次构建方案。
2. 按逗号切分，但 `[...]` 里的逗号不切；`link:` / `dep:` / `feature:` / `version:` 开头的片段并入前一个扩展。
3. 名字后面的 `=` 和 `:` 等价：`curl=link:shared` 和 `swoole:link:shared`。
4. 裸名字使用默认值：内置扩展 → static，非内置扩展 → shared（origin 由 php recipe 的 `builtin_exts` 推导）。
5. `dep:依赖` 打开这个依赖的集成：按 recipe 的 `dep_configure` 给扩展加参数，不向依赖传 feature。
6. `dep:依赖[a,b]` 在打开集成的同时把 `a`、`b` 转给该依赖；feature 之间也可以用 `+`。
7. `feature:[a,b]` 打开扩展自己的编译开关；开关可能在选中时额外引入一个依赖库
   （如 `swoole=feature:[cares]` 会编 `c-ares`）。必须带方括号；`feature:[[]]` 不合法，关闭全部 feature 写 `feature:[]`。
8. `feature:` 可以出现多次，取并集；跨列表重复的 feature 会去重。
9. 条件 feature（如 `argon2`）在版本不满足时直接报错，并说明缺哪一条。
10. `version:线` 指定扩展自己的版本线；依赖库用 `--dep`。
11. 同一个扩展只能出现一次；同一个依赖、同一个 version 只能出现一次。

示例：

```bash
# swoole 走 bundle 的 6.2 线
phprt install 8.5 --ext swoole

# swoole 固定 6.0 线，并把 ssl/nghttp2 转给 libcurl
phprt install 8.2 --ext swoole=version:6.0,dep:libcurl[ssl,nghttp2]

# openssl 打开 argon2（需要 PHP 8.5/8.6 + openssl 3.5/4.0）
phprt install 8.5 --ext 'openssl=feature:[argon2]'

# 已有 state 里默认开了 argon2，想关掉
phprt install 8.5 --ext 'openssl=feature:[]'

# 只装 PHP：禁用 8.5 的两个默认扩展
phprt install 8.5 --disable-default-ext openssl --disable-default-ext curl
```

## 校验与退出

| 情况 | 错误示例 |
| --- | --- |
| 未知扩展 | `未知扩展 ext:nope` |
| 契约不支持的链接方式 | `ext:xdebug 契约不支持 static（支持：shared）` |
| 扩展没有这个依赖集成 | `ext:curl 没有依赖集成 nope（可写：libcurl）；需要在 recipe 的 dep_configure 里声明` |
| 扩展没有这个 feature | `ext:openssl 没有 feature nope（可用：argon2）` |
| feature 写法不对 | `ext:openssl 的 feature: 后面必须写方括号，例如 feature:[argon2]` |
| feature 的 PHP 条件不满足 | `ext:openssl 的 feature argon2 需要 PHP 8.5, 8.6，当前 PHP 8.4` |
| feature 的依赖线条件不满足 | `ext:openssl 的 feature argon2 需要 dep:openssl 3.5, 4.0，当前 3.0` |
| 上游选项未处置 | `N 个上游选项还没处置：\n  ext:openssl --with-system-ciphers ...` |
| 依赖没有这个 feature | `dep:libcurl 契约没有 feature brotli` |
| 版本线不存在 | `「swoole」没有版本线 6.3（可用：4.8, 5.1, 6.0, 6.2）` |
| 组件没有版本表 | `「curl」没有可选版本（catalog 里没有这个组件）` |
| 版本线不支持当前 PHP | `swoole 4.8 不支持 PHP 8.5（可用：6.2）` |
| 显式 pin 冲突 | `openssl 3.5 与当前 PHP/扩展选择冲突（可用：1.1.1）` |
| 禁用非默认扩展 | `--disable-default-ext swoole 不是 PHP 8.5 的默认扩展（默认：openssl, curl）` |
| 同时启用和禁用 | `--ext 和 --disable-default-ext 同时指定了 openssl` |
| 禁用参数用错命令 | `--disable-default-ext 只支持 install / update / plan` |
| 状态文件里有未知 feature | `state.json 里 dep:libcurl 没有 feature brotli` |
| 移除不在方案里的扩展 | `ext:nope 不在构建方案里` |
| 离线且缓存里没有源码包 | `离线模式，缓存里没有这个文件` |
| 源码包下载失败 | `N 个源码包下载失败`（退出码 1，不写状态） |
| 解压时源码包不在缓存 | `源码包不在缓存: ...`（提示先 `--downloads`） |
| 不认识的压缩格式 | `不认识的压缩格式: x.tar.zst（支持 ...）` |
| 压缩包越界路径 | `压缩包里有越界路径: ../x` |
| 解压失败 | `解压 <文件> 失败: ...`（退出码 1，不写状态） |

除 Ctrl-C（130）外，任何错误都退出码 1。
