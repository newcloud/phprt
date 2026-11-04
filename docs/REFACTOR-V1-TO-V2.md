# 从 v1 重构到 v2

记录 phprt 从「全局安装根」模型换成「项目本地自包含」模型的完整过程。
旧版本代码保留在标签 `v1-legacy`，新版本是 `v2.0.0`。

## 1. v1 是什么

| 组件 | 位置 | 作用 |
| --- | --- | --- |
| `install.sh` | 仓库根 | 选版本、选扩展，把 PHP 和依赖装到安装根 |
| `build.sh` | 仓库根 | 一次性构建整个安装根 |
| `config.conf` | 仓库根 | **所有** PHP 版本、依赖版本、URL 集中在这里 |
| `lib/modules/{deps,builtin,ext,tools}/` | 仓库 | 共享 recipe，所有版本共用 |
| `lib/versions/php-<版本>.conf` | 仓库 | 每个 PHP 版本的少量参数 |
| `ini/{dev,prod,default}.ini` | 仓库 | 三套 php.ini profile |
| `phprt` | 仓库根 | `list` / `modules` / `init` / `exec` / `build` / `verify` / `clean` / `home` |

安装根默认是 `~/.phprt`，也支持项目里的 `./.phprt`。项目里的 `myphp` 指向安装根，
运行时用 `LD_LIBRARY_PATH` 把 `deps/*/lib` 串起来。

## 2. 为什么要换

按实际踩到的顺序：

1. **一个安装根只能有一套扩展选择。** A 项目要 redis，B 项目要 swoole，只能重编到同一个根，
   两边互相覆盖；换成项目内 `.phprt` 只是绕开，recipe 仍然是共享的。
2. **recipe 和版本绑死。** 7.4 / 8.5 / 8.6 的差异靠 `config.conf` 的开关和条件分支凑，
   每加一个版本要在多处判断。
3. **运行时靠 `LD_LIBRARY_PATH`。** 整棵树搬走、改名、被别的东西 `exec` 时都要重新拼路径。
4. **同名库冲突。** 系统里已有 `libz.so.1`、`libxml2.so.16`，安装根的版本和系统版本
   在同一个进程里容易互相抢；`LD_LIBRARY_PATH` 是进程级的，没法只影响某一个库。
5. **增量状态分散。** 依赖、PHP、扩展各自用 stamp 文件判断，跨阶段依赖变化时判断不精确，
   经常出现「配置改了但没重编」。

## 3. v2 的模型

一句话：**仓库只做软件源，项目 `.phprt` 自包含。**

| 角色 | 内容 | 是否进项目 |
| --- | --- | --- |
| 仓库 | `phprt` 入口、`runner/`、`bundles/php-<版本>/`、`downloads/`、`docs/` | — |
| 项目 | `.phprt/bin/phprt`、`.phprt/bin/runner/`、`.phprt/bundles/php-<版本>/` | 是，整套复制 |
| 编译产物 | `.phprt/php-<版本>/`（含 `deps/`、`lib/phprt/`、`.phprt-state/`） | 是，项目私有 |
| 源码缓存 | `.phprt/downloads/`（从仓库硬链接，跨盘复制） | 是 |

命令：

```bash
phprt set <版本> [--ext "a b"] [--recompile] [--clean-src] [--plan] [--yes]
phprt current
phprt list
phprt add ext <名字>
phprt remove ext <名字>
phprt verify
```

`set` 负责整条链：解析扩展 → 算依赖闭包和拓扑顺序 → 复制 runner/bundle/源码 →
按 `deps → php → ext` 构建 → 整理运行时库 → 生成 `myphp`。

每一步的签名写在 `<tree>/.phprt-state/`：

| 签名 | 组成 |
| --- | --- |
| `deps/<库>` | recipe 内容 hash + 版本 + 影响产物的开关 + 传递依赖的 recipe |
| `php` | 最终 configure 行 + `php.sh`/`builtin/*.sh` 内容 + 用到的依赖签名 |
| `ext/<扩展>` | recipe hash + 版本 + PHP ABI + 自己的依赖签名 |

签名不变就跳过，所以重复 `set` 是秒级；改一处只重编受影响的那一段。

## 4. 文件对照

| v1 | v2 |
| --- | --- |
| `install.sh` | 删除，`phprt set` 取代 |
| `build.sh`（根） | `runner/build.sh`（复制进项目） |
| `config.conf` | `bundles/php-<版本>/bundle.conf`（按版本拆开） |
| `lib/versions/*.conf` | 合并进各 `bundle.conf` |
| `lib/modules/deps/*.sh` | `bundles/php-<版本>/deps/*.sh` |
| `lib/modules/builtin/*.sh` | `bundles/php-<版本>/builtin/*.sh` |
| `lib/modules/ext/*.sh` | `bundles/php-<版本>/ext/*.sh` |
| `lib/common.sh` | `runner/common.sh`（去掉了安装根相关逻辑） |
| `lib/myphp-project.tmpl` | `runner/myphp.tmpl` |
| `ini/{default,dev,prod}.ini` | 只剩项目级 `.phprt/php.ini` |
| 全局安装根 `~/.phprt` | 删除 |
| 运行时 `LD_LIBRARY_PATH` | 删除，改成 `lib/phprt` + `$ORIGIN` rpath |

命令对照：

| v1 | v2 |
| --- | --- |
| `install.sh --profile minimal` | `phprt set 8.5.12 --ext "..."` |
| `phprt init` | `phprt set` 自动生成 `myphp` |
| `phprt build --only php` | `phprt set 8.5.12 --recompile` |
| `phprt modules` | `phprt list` / `phprt current` |
| `phprt exec -- ...` | `./myphp ...` |
| `phprt home` | 无（路径固定在项目 `.phprt`） |

## 5. 用户迁移步骤

```bash
# 1) 旧项目：旧 .phprt 不能复用，直接删掉重建
cd /path/to/project
rm -rf .phprt

# 2) 用新仓库构建（源码缓存会从仓库 downloads/ 硬链接，不用重新下载）
/path/to/phprt/phprt set 8.5.12 --ext "openssl zlib curl sqlite redis" --yes

# 3) 验证
./myphp -v
./myphp -m
./.phprt/bin/phprt verify
```

`myphp` 会被重新生成，新增 `--shell`：

```bash
eval "$(./myphp --shell)"   # 之后直接敲 php / composer
```

## 6. 不兼容点

- 没有 `install.sh`、没有全局安装根，`phprt exec`/`build`/`home` 已移除。
- 旧 `.phprt` 目录不能升级，只能重建。
- 多 profile（dev/prod）不再内置，只有一个 `.phprt/php.ini`；需要多套就自己复制切换。
- 旧的共享 recipe 不兼容，新 recipe 用 `bundles/<版本>/` 下的写法（见 `docs/ADD-EXTENSION.md`）。
- 依赖库不再位于 `deps/*/lib` 供运行时使用；运行时只认 `lib/phprt`（见 `docs/WHEEL-LIBS.md`）。

## 7. 实施记录

```
40ed613  feat(mini): wheel 化运行时库（hash SONAME + $ORIGIN rpath）
13ed979  feat!: 切换为项目本地自包含模型（v2）
3d5d17d  merge: 用项目本地自包含模型替换旧版本（v2）   ← main / v2.0.0
```

标签：

- `v1-legacy` → `40ed613`：旧模型最后一个可用状态
- `v2.0.0` → `3d5d17d`：新模型

删除的文件：`install.sh`、`build.sh`、`config.conf`、`ini/`、`lib/`、旧 `phprt`、
`docs/MODULES-HISTORY.md`、`docs/SANDBOX-HISTORY.md`。

新增/改写：`runner/`、`bundles/`、`README.md`、`docs/ADD-EXTENSION.md`、`docs/PROJECT-LOCAL.md`、
`docs/PHP-VERSIONS.md`、`docs/WHEEL-LIBS.md`、`.gitignore`。

## 8. 验证结果

新仓库、全 17 个扩展、真实项目（RoadRunner）：

```text
phprt set 8.5.12 --ext "openssl zlib libxml2 curl sqlite mysqli mbstring intl
                        zip gd ffi redis protobuf xdebug apcu yaml swoole" --yes   → ok

env -i ./myphp -m
  → openssl zlib dom xml simplexml xmlreader xmlwriter soap curl sqlite3 pdo_sqlite
    mysqli pdo_mysql mbstring intl zip gd ffi redis protobuf xdebug apcu yaml swoole

env -i ./myphp -r '...'        → 17 项功能全部正常（见 WHEEL-LIBS.md 的实测数据）
./.phprt/bin/phprt verify      → 4 步全过
./bin/rr serve -c .rr.yaml     → HTTP 200
```

## 9. 遗留

- 7.4 仍然只有最小档（`libxml2` + 内核基础扩展），PECL 没适配。
- 没有 bundle 版本升级/旧依赖清理机制；换版本要 `set` 新的 bundle。
- `verify` 不检查 bundle 完整性，只检查运行时（加载、`ldd`、RPATH）。
- 旧的共享 recipe 能力（多 profile、`phprt exec`）没有平移，需要时再单独设计。
