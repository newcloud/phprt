# 扩展与依赖的 feature

本文件是新增或修改 recipe 时必须走的流程。目标只有一条：**上游有哪些开关，是 agent 主动盘点并逐个处置的，不是等人来提醒。**

处置只有三种，都要在 recipe 里留下痕迹：

| 处置 | 写法 | 什么时候用 |
| --- | --- | --- |
| 固定 | `configure` / `dep_configure` | recipe 自己决定，用户不需要选 |
| 暴露 | `features`（扩展自己 / 依赖库） | 用户可能想开或关 |
| 暴露且带依赖 | `features.<名字>.needs` | 开关需要额外的库，只在该开关选中时引入 |
| 不暴露 | `ignored` + 理由 | 看过，但这次不提供 |

## 1. 流程

```bash
# 1. 先解压一次源码（audit 读上游的 config.m4 / configure --help）
phprt install 8.5

# 2. 对照上游选项清单
phprt --php 8.5 audit

# 3. 逐个处置：写进 features / dep_configure，或写进 ignored 并给理由

# 4. 再跑一次，直到没有「未处理」
phprt --php 8.5 audit

# 5. 决定默认值，写进 bundles/php<系列>.toml
```

`audit` 的输出形态：

```text
ext:openssl
  source  build/php/php-8.5.12/ext/openssl
  --with-openssl                     configure
  --with-openssl-argon2              feature argon2
  --with-openssl-legacy-provider     ignored: 会改变默认 TLS 行为，需要单独验证
  --with-system-ciphers              ignored: 依赖系统 cipher 配置，构建不可复现
  feature argon2                   默认开  requires php 8.5/8.6，dep:openssl 3.5/4.0
  feature jpeg                     预设开  --with-jpeg
```

- 扩展 recipe 有「未处理」→ 命令失败（退出码 1）。
- `默认开` 表示扩展默认安装时就启用；`预设开` 表示只在 `--ext 名字` / `ext add 名字`
  时由 `[ext_presets]` 启用，不会让扩展默认安装。
- 依赖库只报数量，不失败：库的绝大多数选项由 recipe 固定，不需要全部做成 feature。
- `ignored` 里写了上游不存在的选项 → 也算未处理（防止拼写错误）。
- 源码没解压时会提示先安装一次，不会假装通过。

## 2. 扩展 feature 与依赖 feature 的区别

| | 扩展 feature | 依赖 feature |
| --- | --- | --- |
| 归谁 | PHP 扩展自己 | 依赖库 |
| 怎么开 | `--ext 扩展=feature:[名字]` | `--ext 扩展=dep:依赖[名字]` |
| 声明位置 | ext 的 `features` | dep 的 `features` |
| 例子 | openssl 的 `argon2` | libcurl 的 `ssl` / `nghttp2` |

依赖库的 `features.*.configure` 是该 feature 打开时给**库**的编译参数；`disable` 是没打开时给库的
关闭参数。扩展的 `dep_configure` 是选中 `dep:依赖` 时给**扩展**加的编译参数。

## 3. 常用度判定

盘点的第一道筛子是常用度，不是「能不能做」。命中即停：

1. **常用 / 有明确需求** → 开发。需要新依赖就先加 dep recipe，把依赖写进 feature 的
   `needs`，不要因为麻烦就记成 `ignored`。
2. **少用** → `ignored`，理由以 `少用：` 开头，写清场景。
3. **只影响调试、覆盖率、开发者模式** → `ignored`，理由以 `调试用途：` 开头。
4. **常用但当前缺依赖** → `ignored`，理由以 `缺依赖（未排期）：<库名>` 开头，
   让它在下一次盘点里能被一眼找出来，而不是混在“少用”里。

按这个筛子判断之后的处置：

- 影响产物行为、用户可能有不同选择 → 做成 feature。
- 需要新库且常用 → 做成 feature + `needs` + 新 dep。
- 其余 → 在 `configure` / `dep_configure` 里固定。

依赖库的 feature 只做「消费扩展可能请求的开关」，典型是外部库集成：`ssl`、`nghttp2`、
`brotli`、`zstd`、`idn`。不要为列出全部 182 个 libcurl 选项而做 182 个 feature。

### 3.1 当前判定

| 上游选项 | 判定 | 落地 |
| --- | --- | --- |
| openssl `--with-openssl-argon2` | 常用，依赖已在编 | feature，默认开 |
| openssl `--with-openssl-legacy-provider` | 有需求（老证书、MD4/RC4/DES） | feature，默认关 |
| openssl `--with-system-ciphers` | 少用 | ignored |
| swoole `--enable-cares` | 常用（异步 DNS） | feature + `needs=["c-ares"]`，默认关 |
| swoole `--enable-brotli` | 有一定需求 | 缺依赖（未排期）：brotli |
| swoole `--enable-zstd` | 较新、较少 | 缺依赖（未排期）：zstd |
| libxml2 `--with-zlib` / `--with-icu` / `--with-readline` / `--with-python` | PHP 的 libxml 用不到 | 固定关闭，避免探测系统库 |
| swoole iouring / liburing | 性能向，依赖内核 | 少用 |
| swoole mysqlnd / sockets / pgsql / sqlite / odbc / oracle / firebird / ssh2 / ftp | 少用，且要先有对应 PHP 扩展 | 少用 |
| swoole asan / debug / debug-log / trace-log / coverage / dev / coro-time / thread | 调试或开发用 | 调试用途 |
| xdebug `--with-xdebug-compression` | 少用 | 缺依赖（未排期）：zlib |
| xdebug `--enable-xdebug-dev` | 开发用 | 调试用途 |
| gd `--with-jpeg` | 常用 | feature `jpeg`，默认关；需要时显式打开 |
| gd `--with-webp` | 常用 | feature `webp`，默认关 |
| gd `--with-freetype` | 常用 | feature `freetype`，默认关 |
| gd `--with-avif` | 常用但依赖较大 | feature `avif` + `needs=["aom","avif"]`，默认关 |
| gd `--with-external-gd` / `--with-xpm` / `--enable-gd-jis-conv` | 少用 | ignored |
| libjpeg-turbo `WITH_SIMD` | 常用（性能） | dep feature `simd`，默认关；`requires.commands=["nasm"]` |

PNG 不是 feature：PHP 8.5 gd 强制依赖 libpng，`--enable-gd` 就带上。

### 3.2 宿主机命令条件

feature 可以要求 PATH 里存在某个命令：

```python
"simd": feature(
    configure = ["-DWITH_SIMD=ON", "-DREQUIRE_SIMD=ON"],
    disable = ["-DWITH_SIMD=OFF"],
    requires = {"commands": ["nasm"]},
)
```

- 显式选择不满足时在构建前报错，并提示安装哪个命令。
- bundle 默认不满足时提示并丢弃，不中断安装。
- `needs` 只用于 dep recipe，不能把 `nasm` 写进 `needs`。

## 4. 要不要默认开启

默认值只写在 `bundles/php<系列>.toml`，不写进 recipe：

- `[default_exts]`：默认安装哪些扩展；
- `[ext_presets]`：扩展被选中时默认开哪些 feature，不决定是否默认安装。

四条判据，从强到弱：

1. **不增加新依赖、不改变行为，只是多一个可用能力 → 默认开。**
   例：openssl 的 `argon2`，OpenSSL 已经在编，只是多一个 KDF。
2. **依赖已经在默认集里 → 默认开，并且必须和依赖一起写。**
   例：`curl = { deps = { libcurl = ["ssl"] } }`，libcurl 本来就要编，ssl 不额外增加组件。
3. **改变默认行为或安全边界 → 默认关。**
   例：`--with-openssl-legacy-provider`。
4. **上游默认关闭、只在特定场景用 → 默认关。**
   例：swoole 的 `debug-log`、`swoole-coverage`、`asan`。

补充规则：

- 默认开启后用户无法直接“只关这一个”时，必须先确认 `feature:[]` 能清空（当前行为）。
- 条件 feature 用 `requires` 声明；默认集里写了一条在某个 PHP 系列不满足的条件，
  命令会提示并丢弃，不会中断。
- 一件事只有一种判据：不要因为“可能有人要”就默认开。

## 5. 常见错误

- 把常用开关一律写成 `ignored`，让 audit 通过 → 这是把驱动当过关，不算做完。
- 只加 `ignored` 不写理由 → 加载时报错。
- `ignored` 里写上游没有的选项 → `audit` 报未处理。
- 把调试开关默认打开 → 默认集里出现 `debug` / `asan` / `coverage` 类开关。
- 把需要新依赖的开关默认打开，但依赖没进默认集 → 安装后功能悄悄不可用。
- 在 `target(ctx)` 里用 `if` 判断版本、悄悄不暴露开关 → 应改用 `requires`。
- 默认值写进 recipe → bundle 才是默认值的唯一来源。

## 6. 带依赖的 feature

开关需要额外库时，写 `needs`，不要在 `needs` 里无条件加库：

```python
features = {
    "cares": feature(
        needs = ["c-ares"],
        configure = ["--enable-cares"],
    ),
}
```

- 只有选中这个 feature 时，依赖才进构建顺序和链接闭包。
- 依赖本身要按第 1～4 条判断默认是否开启；上面这个 cares 默认关。
- 依赖库用 pkg-config 找自己的位置时，`PKG_CONFIG_PATH` 已经包含它的前缀。

## 7. 验收

```bash
cargo fmt --all
cargo test --workspace
phprt --php 8.5 audit        # 必须退出码 0
phprt --php 8.5 plan         # 检查 features / dep 行和 configure 参数

# 带依赖的 feature：选和不选各看一次构建顺序
phprt --php 8.5 plan                        # 不应出现 dep:c-ares
phprt --php 8.5 --ext 'swoole=feature:[cares]' plan
                                            # 出现 dep:c-ares，swoole configure 带 --enable-cares
```

真构建至少要覆盖一次默认集：`phprt install 8.5`，并确认默认开启的能力在
`php -m` / `php -i` 或对应的 `php -r` 检查里可见。
