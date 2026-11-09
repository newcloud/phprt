# phprtv2 开发任务

本文记录 phprtv2 的定位、已完成的基础、以及后续开发任务。任务按依赖顺序排列，
每条都写了验收方式。设计有分歧的地方集中放在最后一节，不在这里下结论。

## 1. 定位

phprtv2 用来验证一套分层：

- `bundles/php<系列>/**` 是**数据**：版本、下载地址、依赖关系、编译步骤、ini 内容。
- `runner/*.sh` 是**驱动**：解析数据、算拓扑、管签名/目录/锁/安装树，不认识具体包名。

验收标准只有一条：**加一个扩展或依赖，只改 `bundles/`，`runner/` 零改动**，
`phprt verify` 仍然全绿。

## 2. 已完成的基础（v2 当前状态）

| 能力 | 实现位置 | 说明 |
| --- | --- | --- |
| 版本树 | `.phprt/php-<版本>/` | 标准 PHP 布局（`bin/etc/lib/deps`），可整体搬移 |
| 构建区按版本分 | `.phprt/build-php-<版本>/` | `src/logs/tools/downloads`，整目录可删 |
| 精确版安装 | `phprt install 8.5.11` | 不复制 bundle，靠 `apply_php_override` 换版本信息 |
| 扩展状态 | `runner/common.sh:tree_exts` | 不落盘，按 `myphp` 环境跑 `php -m` |
| 签名增量构建 | `runner/build.sh` + `.phprt-stamp-*` | 依赖 / PHP / ext 三层签名 |
| 校验 | `phprt verify` | PHP、扩展、动态库、RPATH、配置路径、CA、wheel |
| 离线与缓存 | `runner/common.sh:find_source/ensure_source` | 仓库 `downloads/` 为种子，缺包下载到本版本 `build-php-*/downloads/` |

这些是后续任务的基座，暂时不动。

## 3. 数据与驱动的边界（后续任务必须遵守）

driver 只能通过 recipe 的头部字段和标准函数取信息，不能出现具体包名。
当前已支持的字段见 `docs/RECIPE.md`：`kind/name/version/switch/modules/ini/needs/optional/tools/wheel`。

本轮要新增的字段和函数（都属于「数据」）：

### 3.1 recipe 头部字段

```text
# phprt: ... upstream=github:owner/repo
# phprt: ... upstream=pecl:swoole
# phprt: ... upstream=phpnet:8.5
# phprt: ... upstream=custom
# phprt: ... desc="Redis 缓存客户端"
# phprt: ... opts=jpeg,png,webp,avif,freetype
```

| 字段 | 作用 |
| --- | --- |
| `upstream` | 版本发现来源。常见来源不用写函数，driver 调公共实现 |
| `desc` | 一行说明，交互选择器显示；不参与构建 |
| `opts` | 可选子开关（如 gd 的图片格式），供选择器生成二级菜单 |

### 3.2 recipe 可选函数（覆盖默认实现）

| 函数 | 输入 | 输出 | 何时需要 |
| --- | --- | --- | --- |
| `versions` | 无 | 一行一个稳定版版本号 | `upstream=custom` 或来源不在公共实现里；优先用这个 |
| `latest_version` | 无 | 单个最新稳定版 | 只关心最新版时的简写 |
| `download_url <版本>` | 版本号 | 完整下载地址 | URL 规则无法用模板表达时 |
| `checksum <版本>` | 版本号 | sha256（没有则空） | 上游提供校验和时 |
| `ini_body` | 无 | 追加到自动生成的 ini 后面 | 扩展有专属 ini 指令；有独立 `ini=` 的仍可用 `enable()` |

### 3.3 公共实现（driver 侧，新增）

新建 `runner/upstream.sh`，只放「同一类上游怎么查」的通用逻辑：

```text
upstream_latest_github <owner/repo> [--prerelease]
upstream_latest_pecl <package>
upstream_latest_php <大版本.小版本>
upstream_latest_cpan <package>
http_get <url>                       # 复用 PHPRT_PROXY、超时、User-Agent
cache_get / cache_put <key> <ttl>    # ~/.cache/phprt/upstream，默认 6 小时
version_cmp <a> <b>                  # 语义化比较，处理 -dev/-rc
version_is_safe <当前> <候选>         # 只接受 major.minor 相同、patch 变大
```

约束：

- driver 不写包名；包名只出现在 recipe 的 `upstream=` 里。
- 离线模式（`OFFLINE=1`）或命令带 `--no-update-check` 时**不发起任何请求**，直接用本地 pin / 历史。
- 允许查询但查询失败（断网、限流、超时）时回退到 `bundle.conf` 的 pin，并打印原因。
- GitHub API 支持 `GITHUB_TOKEN`；未设置时按匿名额度限速并缓存。
- 版本比较不用 `sort -V` 兜底，单独实现，覆盖 `8.6.0-dev`、`-rc1`。

## 4. M1：在线版本发现与安全更新检测

### 4.1 任务

1. 新建 `runner/upstream.sh` 和上面的公共函数。
2. recipe 头部支持 `upstream=` / `desc=` / `opts=`；`recipe --meta` 一并输出。
3. 给现有 recipe 补 `upstream=`：
   - PHP 本体：`bundle.conf` 增加 `PHP_UPSTREAM=phpnet:8.5`
   - openssl / zlib / curl / libzip：各自源站
   - swoole：`pecl:swoole`
   - autoconf / perlshim：GNU FTP / CPAN
4. 补齐现有 pin 的 `*_SHA256`（当前除 PHP 外都是空）。下载时已有的 `sha_check` 要真正生效。
5. 新增 `phprt outdated [--json] [--all] [系列]`：

   ```text
   模块        类型     已装      安全更新   最新
   php         core     8.5.12    -          8.5.12
   openssl     dep      3.5.9     -          4.1.0
   curl        dep      8.23.0    -          8.23.0
   swoole      ext      6.2.4     6.2.5      6.2.5
   ```

   - **默认只检查当前版本树里实际启用的扩展及其依赖**（`php -m` 得到的扩展，
     再加上 `needs`/`tools` 的递归闭包）。没启用的扩展/依赖不查询、不提示，
     结尾打印「只提示当前 php.ini 配置了的扩展及依赖」。
   - `--all`：检查 `bundles/` 里所有已接入的模块（含没启用的），用于评估升级面。
   - `--json` 供后续 UI 或 CI 使用。
   - 支持代理：`https_proxy` / `HTTPS_PROXY`，没有时用 `http_proxy` / `HTTP_PROXY`
     （HTTPS 请求也走它）；代理失败会尝试直连，最后才用 `PHPRT_PROXY`。
   - 退出码：查询成功为 0（有没有更新都是 0），网络/上游不可用为 3。
6. 新增契约门禁：
   - `scripts/recipe-check.sh`：开发用脚本，静态校验全部 recipe；`--deep` 再联网确认
     上游能查到版本、pin 在版本列表里。加扩展/依赖后必须跑通。
   - 不新增 `phprt recipe` 子命令：这是开发门禁，不是用户功能。

### 4.2 「安全更新」的定义

- 三段及以上版本（`x.y.z`）：只接受 `major.minor` 相同、`patch` 变大（8.5.12 → 8.5.13）。
- 两段式版本（`x.y`，如 autoconf 2.72 → 2.73）：只接受 `major` 相同、第二段变大。
- 一段式版本不做安全更新判断。
- 忽略预发布：`-dev`、`-alpha`、`-beta`、`-rc`。
- 跨小版本（8.5 → 8.6、curl 8.23 → 8.24）不算安全更新，只提示，需要显式 `--minor`。
- 不允许降级。

### 4.3 验收

- 断网跑 `phprt outdated`：条目显示 `-`，结尾提示结果不完整，退出码 3，不报堆栈。
- 默认范围：停用 swoole（删 `conf.d/27-swoole.ini`）后，`swoole` 和只被它需要的工具不再出现；
  `--all` 仍然包含。
- `http_proxy` 指向可用代理、`https_proxy` 未设置时，HTTPS 查询也能成功；
  代理端口不通时会自动尝试直连。
- `OFFLINE=1 phprt outdated`：不发起请求，打印「离线模式，未查询上游」，退出码 3。
- `OFFLINE=1 phprt install 8.5` 与 `--no-update-check` 行为一致：不进提示、不联网。
- 伪造上游响应（本地 HTTP 服务或 `PHPRT_UPSTREAM_FIXTURE`）能稳定复现「有更新 / 无更新 / 预发布」三种情况。
- `scripts/recipe-check.sh` 对缺 `upstream` 的 recipe 直接判失败。

### 4.4 已实现（2026-11-08）

- `runner/upstream.sh`：GitHub tags / PECL stable / php.net / 目录索引，带缓存、代理、
  `OFFLINE` 短路；`version_cmp` / `version_is_safe_update`。
- recipe：`upstream=`（openssl / curl / libzip / swoole）+ `versions()`（zlib / autoconf / perlshim）。
- `bundle.conf`：`PHP_UPSTREAM="phpnet:8.5"`。
- 命令：`phprt outdated [--all] [--json] [系列]`。`OFFLINE=1` 是全局离线模式，
  不属于 `outdated` 的参数；离线时 `outdated` 只报告“未查询”，退出码 3。
- 默认范围 = 当前树的 `php -m` 扩展 + `needs`/`tools` 闭包；`--all` 才遍历全部 recipe。
  代理顺序：用户代理（https_proxy → http_proxy）→ 直连 → `PHPRT_PROXY`。
- 契约门禁：开发脚本 `scripts/recipe-check.sh [--deep]`，不挂 `phprt` 子命令。
  静态检查覆盖字段、switch、version、ini、函数、顶层代码、依赖可解析、upstream 格式；
  `--deep` 还要求上游可查、pin 在列表里。
- 实测（把 pin 临时改成旧版本后检查，再恢复）：

  | 模块 | 旧 pin | 安全更新 | 最新 |
  | --- | --- | --- | --- |
  | php | 8.5.4 | 8.5.12 | 8.5.12 |
  | openssl | 3.5.4 | 3.5.9 | 4.1.0 |
  | zlib | 1.3.1 | 1.3.2 | 1.4 |
  | libzip | 1.12.0 | 1.12.1 | 1.12.1 |
  | swoole | 6.2.3 | 6.2.5 | 6.2.5 |
  | autoconf | 2.71 | 2.73 | 2.73 |
  | perlshim | 5.42.0 | 5.42.3 | 5.44.0 |
  | curl | 8.22.0 | -（8.22→8.23 跨小版本） | 8.23.0 |

- 还缺：各 pin 的 `*_SHA256` 仍为空；`install` 还没接入这套检查（M2）。

## 5. M2：update 与安装时的更新检查

### 5.1 更新记录：`php-<版本>/UPDATE-HISTORY`

更新结果写在当前版本树里，不写仓库 `bundle.conf`，也不在 `.phprt` 顶层新增文件：

```text
.phprt/php-8.5.12/UPDATE-HISTORY
```

位置在版本树根目录，和 `BUILD-INFO` / `BUILD-STATUS` 同级，属于 phprt 元数据。

格式（只追加，不改写）：

```text
2026-11-08T21:32:10+08:00  libzip  1.12.1 -> 1.12.2  安全更新
2026-11-08T21:35:02+08:00  swoole  6.2.4  -> 6.2.5   安全更新
2026-11-08T21:40:11+08:00  php     8.5.12 -> 8.5.13  安全更新
```

规则：

- 同一模块以**最后一条**记录为准，作为构建时的版本覆盖，优先级高于 `bundle.conf` 的 pin。
  这样重复执行 `install` 不会把已更新的库退回旧版本。
- `bundle.conf` 始终是「默认值 / 数据源」，`UPDATE-HISTORY` 是「这个项目实际采用的版本」。
- PHP 自身升级会创建新树 `php-8.5.13/`：先复制旧树的 `UPDATE-HISTORY`，再追加 PHP 这一条，
  历史跟着项目走，不会因为换树丢失。
- 文件随版本树一起搬移、一起删除；`.phprt` 顶层仍然只有 `current`、`php-*`、`build-php-*`。

### 5.2 `install` / `add` 的检查流程

默认行为：**凡是这次要用的模块，都先判断一次版本**。检查范围按命令区分：

| 命令 | 是否检查 | 检查范围 | 跳过方式 |
| --- | --- | --- | --- |
| `install 8.5`（系列） | 默认检查 | PHP + 本次构建计划拓扑里的全部依赖、扩展、工具 | `--no-update-check` / `OFFLINE=1` |
| `install 8.5.12`（精确） | 不检查 | 用户明确指定版本，直接装 | — |
| `add ext <名字>` | 默认检查 | 新扩展本身 + 它的 `needs` / `tools`（递归） | `--no-update-check` / `OFFLINE=1` |
| `update` | 检查 | 指定模块，或全部安全更新 | — |

说明：

- 「构建计划拓扑」和 `build.sh` 解析出来的 `ALL_DEPS` 一致：只查这次真正会构建的模块，
  bundle 里没被选中的依赖不查、不提示。
- `add ext redis` 只查 redis 和它依赖的库（如 openssl/zlib），不会把已启用的其他扩展
  重新问一遍。
- `--plan` 也执行检查，并在计划里标出「将使用 1.12.2（安全更新）」；
  `--no-update-check` 时计划只显示 pin / 历史版本。
- 检查结果里 PHP 有更新时走下面的退出流程；依赖/扩展有更新时逐个询问。

流程（`install` 系列 / `add`）：

1. 解析 pin 得到具体版本（如 8.5.12）。
2. 按上面的范围收集本次要用的模块，逐个执行 online 检查（复用 `outdated` 的内部逻辑）。
3. **PHP 有安全更新时**：不进入构建，打印提示后退出：

   ```text
   bundle.conf 中的 PHP 版本已旧：8.5.12 -> 8.5.13（安全更新）
   请执行：phprt install 8.5.13
   不想检查更新可加：--no-update-check（或 --offline）
   ```

   退出码 2（检测到更新、未安装）；`phprt outdated` 只做检查，查到更新也返回 0，
   网络失败返回 3。PHP 的这条提示不受 `--yes` 影响：换 PHP 版本会新建一棵版本树，
   必须由用户显式执行。

4. **依赖 / 扩展有安全更新时**：逐个询问，默认 `[Y]`：

   ```text
   bundle.conf 中的 libzip 版本已旧：1.12.1 -> 1.12.2（安全更新）
   采用 1.12.2？[Y/n]
   ```

   - 回 `y` / 回车：本次构建使用新版本，成功后追加到 `UPDATE-HISTORY`。
   - 回 `n`：本次仍用 pin，不写历史。

   `add ext` 只问新扩展和它的依赖；PHP 本身不检查，不会因为上游有新版就提示换版本。

5. 参数与模式：

   | 参数 / 模式 | 作用 |
   | --- | --- |
   | `--no-update-check` | 本次完全不查上游，直接用 pin / 现有历史 |
   | `--offline` / `OFFLINE=1` | 离线模式，与 `--no-update-check` 等价：不做任何自动查询 |
   | `--yes` | 用于无人值守：所有提示按默认回答，即**采用**安全更新 |
   | `--plan` | 只打印计划；计划里显示「将使用 1.12.2（安全更新）」 |

   `--yes` 会采用安全更新，版本可能随上游变化，CI 里需要可复现时用
   `--no-update-check` 或 `OFFLINE=1` 固定到 pin / 历史。

6. 只有真正发起查询时才可能遇到网络问题：打印一行「上游不可用，按 bundle.conf / 历史版本继续」，
   继续构建，不阻断。`--no-update-check` / `OFFLINE=1` 下不会发起请求，也就没有这条提示。

### 5.3 `phprt update`

```text
phprt update                 # 处理所有安全更新（逐个确认，默认 Y）
phprt update swoole          # 只更新指定扩展
phprt update libzip 1.12.3   # 指定版本（不要求是安全更新，需二次确认）
phprt update --minor         # 允许跨小版本，先打印 ABI 风险
phprt update --dry-run       # 只打印计划
```

- 显式版本和 `--minor` 会写入 `UPDATE-HISTORY`，带「手动指定」标记。
- 回滚：`phprt update --rollback <模块>` 追加一条反向记录（保留完整历史，不删行）。

### 5.4 验收

- 8.5.12 升 8.5.13：新建 `php-8.5.13/`，历史复制并追加；只重编 PHP 和受影响的扩展，
  依赖源码复用，不重新解压。
- swoole 6.2.4 升 6.2.5：只重编 swoole，PHP 不重编；`UPDATE-HISTORY` 多一行。
- 更新后重复 `phprt install 8.5`：按历史里的 6.2.5 构建，不回退到 6.2.4。
- 更新后 `phprt verify` 全绿，`php -m` 里扩展仍在。
- `phprt install 8.5.11` 明确旧版本：不提示、不退出，直接装 8.5.11。
- `phprt install 8.5 --no-update-check`：不联网、不进提示；`OFFLINE=1` 行为相同。
  实现上复用 `runner/common.sh` 里已有的 `OFFLINE` 变量，不再引入第二个开关。
- `phprt add ext <名字>`：默认只查该扩展及其依赖；`--no-update-check` / `OFFLINE=1` 跳过。
- `phprt install 8.5`：只查本次计划里的模块，bundle 里未选中的依赖不出现在提示里。

### 5.5 离线模式与内网安装

目标：内网机器提前放好源码包后，`OFFLINE=1 phprt install 8.5` 全程不联网。

规则：

1. `OFFLINE=1` / `--offline` 同时关闭两件事：
   - 上游版本查询（与 `--no-update-check` 相同）
   - 源码下载（`ensure_source` 不允许走 `curl`）
2. 源码只从本地查找，顺序沿用现有实现：

   ```text
   PHPRT_SEED
   → phprt 仓库 downloads/
   → .phprt/build-php-<版本>/downloads/
   → ~/.cache/phprt/downloads/
   → ~/.phprt/downloads/（旧缓存）
   ```

3. 缺包时**不进入构建**：一次性列出缺的文件名和查找过的目录，退出码 1。
   提示里直接给补救方式：

   ```text
   离线模式缺少源码包：curl-8.23.0.tar.xz
   请放到以下任一目录后重试：
     /path/to/phprt/downloads/
     /path/to/project/.phprt/build-php-8.5.12/downloads/
   或设置 PHPRT_SEED=/共享盘/phprt-downloads
   ```

   不允许「装到一半才发现缺包」，所有源码包在开始编译前一次性确认。

4. 系统依赖不在 phprt 范围内：`gcc`、`make`、`cmake`、`pkg-config`、`patchelf`、
   内核头文件等由用户自己准备（内网用 dnf/apt 的离线包即可）。phprt 只在缺命令时报
   `缺少命令: xxx`，不负责安装系统包。

5. 已装好的版本树可以直接搬进内网：把 `.phprt/php-<版本>/` 和 `current` 一起拷过去，
   连编译都不需要。

验收：

- 断网 + downloads 齐全：`OFFLINE=1 phprt install 8.5` 成功，日志里 0 次下载。
- 故意拿走一个包：安装开始前报缺失清单，退出码 1，没有产生半成品版本树。
- `PHPRT_SEED=/共享盘` 指向内网共享目录时，优先从那里取包。

## 6. M3：把更多扩展迁到 v2

原则仍然是一条 recipe 一个文件，不改 driver。建议分三批，每批独立验收。

### 6.1 第一批：纯 PECL，依赖少

| 扩展 | 额外依赖 | 备注 |
| --- | --- | --- |
| `apcu` | 无 | 需要 `--enable-apcu`，无外部库 |
| `igbinary` | 无 | 序列化 |
| `msgpack` | 无 | 序列化 |
| `redis` | 无 | 现有 v1 recipe 可参考 |
| `xdebug` | 无 | 需要 `zend_extension=`，用 `ini_body` 或独立 `enable()` |
| `yaml` | libyaml | 新增 dep recipe |
| `protobuf` | protobuf | v1 有 recipe，注意 C++ 与 `protoc` |

### 6.2 第二批：图像链（gd）

- builtin `gd`：`--with-jpeg --with-png --with-webp --with-avif --with-freetype`
- 新增 deps：`libjpeg-turbo`、`libpng`、`libwebp`、`libavif`、`freetype`（`libaom` 视 avif 需要而定）
- `opts=jpeg,png,webp,avif,freetype`，选择器按 `opts` 生成二级菜单，不写死在 UI 里
- 可选依赖用 `optional=` 表达，未选中的格式不参与链接

### 6.3 第三批：数据库与国际化

| 扩展 | 额外依赖 |
| --- | --- |
| `mongodb` | 无（自带驱动） |
| `imagick` | ImageMagick（v1 有 recipe） |
| `intl` | `icu4c` |
| `mbstring` | `onig` |
| `mysqli` / `pdo_mysql` | 无（builtin，需 mysqlnd） |
| `sqlite3` / `pdo_sqlite` | `sqlite-amalgamation` |
| `xsl` | `libxslt` |
| `sodium` | `libsodium`（或 PHP 内置） |
| `ffi` | `libffi` |
| `gettext` / `bz2` | 系统库或自带 recipe |

### 6.4 验收

- 新增/修改 recipe 后 `scripts/recipe-check.sh` 通过；联网环境再跑一次 `--deep`。
- 每批完成后：`phprt install 8.5.12 --ext "<批量清单>"` 从空 `.phprt` 成功。
- `phprt verify` 全绿；每个动态库都能通过 wheel 检查。
- 对比 `runner/` 目录，除通用逻辑外无非预期改动。

## 7. M4：交互式扩展选择

### 7.1 原型的问题（`gum-test3.sh` demo）

1. `read_key` 通过 `echo` 返回结果，每次按键起一个子 shell，按键延迟明显；用 0.05s 超时区分 ESC 和方向键不可靠。
2. 每帧 `clear` 全屏重绘，闪烁；长列表没有滚动窗口。
3. gd 的格式列表写死在 UI 里，违反「数据 vs 驱动」：格式应当来自 recipe 的 `opts=`。
4. 子菜单按 Esc 时没有回滚草稿，gd 的修改仍然留在全局状态里。
5. 没有依赖联动：选中 gd 是否要自动带上 freetype/libpng，未做提示或约束。
6. 只区分「选中」，没有「已安装（`php -m` 里有）」和「本次新增」的区分。
7. 没有非交互回退，CI 或管道执行时直接报错。

### 7.2 目标形态

新增 `phprt select [--out <文件>]`：

- 数据来源：`recipe --meta` + `desc=` + `opts=` + `needs/optional`；已安装状态来自 `php -m`。
- 分三栏或分组：`builtin` / `deps`（只读展示）/ `ext`（可选）。
- 键位：`↑↓` 移动、`Space/Tab` 开关、`Enter` 进入 `opts` 子菜单、`a` 全选、`/` 过滤、`q` 退出确认。
- 渲染：alternate screen（`tput smcup/rmcup`），只重绘变化行，带滚动窗口。
- 输出契约：选中的扩展一行一个写到 stdout（或 `--out`），提示信息全部走 stderr；退出码 0/1/130。
- 非 TTY：自动退化为编号列表；`--yes` 直接用 `DEFAULT_EXTS`。
- 测试钩子：`PHPRT_KEYS="DOWN SPACE ENTER"` 驱动按键序列，便于脚本化验收。

### 7.3 实现选择

优先纯 bash + `tput`（零外部依赖）。如果维护成本过高，再评估：

- `dialog` / `whiptail`：系统常见，但样式受发行版影响。
- `gum`（charmbracelet）：交互好，但引入外部二进制，离线环境不可用。

无论选哪种，UI 都放在 `runner/ui.sh`，`command` 只调用 `ui_select`。

### 7.4 验收

- 选择结果可以直接喂给安装：`phprt install 8.5 --ext "$(phprt select)"`。
- 选中 gd 时自动列出/勾选它需要的图像库。
- 无法关闭已编译进 PHP 的 builtin 时给出明确提示，而不是静默忽略。
- `PHPRT_KEYS` 脚本化用例覆盖：全选、子菜单、Esc 取消、确认退出。

## 8. M5：buildroot / chroot

v1 的 standalone `phprt buildroot` 已移植到 v2，并补上自动 `--chroot` 构建。

- 实现：`scripts/buildroot.sh`（入口转发已在 `phprt`）。
- rootfs：`<项目>/.phprt/buildroots/<名字>/`，配置在 `.phprt/buildroots/.config`。
- 默认绑定项目 + phprt 仓库；自动构建额外绑定 `BUILD_ROOT` 等。
- rootfs 在项目内时，chroot 内会通过项目 bind 再看到一次；构建只使用精确路径，不做递归扫描。
- `install` / `add ext` 支持 `--chroot` / `--host`；`--plan` 不进 chroot。
- 构建用户默认用 `chroot --userspec=<宿主uid:gid>`，产物属主不变。
- 详见 [CHROOT-BUILD.md](CHROOT-BUILD.md)。

验收：

- `phprt buildroot create/enter/exec/status/remove` 可用，退出后挂载为 0；
- `phprt install --chroot` 产物在宿主 `phprt verify` 全绿；
- `--host` 切回后宿主构建无回归。

## 9. 任务顺序与里程碑

| 里程碑 | 内容 | 依赖 | 完成标准 |
| --- | --- | --- | --- |
| M0 | 分层基线、按版本构建区、`php -m` 状态 | — | 已完成 |
| M1 | upstream 公共层、recipe 字段、`outdated` | M0 | 三种上游来源可查，离线回退正确 |
| M2 | install 时的更新检查 + `phprt update` | M1 | 安全更新可安装、可回退、可验证 |
| M3 | 扩展扩容（三批） | M0 | 每批只改 `bundles/`，verify 全绿 |
| M4 | `phprt select` | M1/M3 | 非交互可测，结果可直接喂给 install |
| M5 | buildroot/chroot | M0 | 已完成：standalone + 自动 `--chroot`，真实构建已验证 |
| M6 | 文档与测试矩阵 | 全部 | README/TEST-V2/RECIPE 同步 |

## 10. 待决问题

### 10.1 `UPDATE-HISTORY` 的读取与回滚细节

已定：记录写在 `php-<版本>/UPDATE-HISTORY`，最后一条覆盖 `bundle.conf` 的 pin。

还需要定：

- 回滚用「追加一条反向记录」还是「允许删除/截断最后一条」。文档先按追加实现。
- 手工编辑 `UPDATE-HISTORY` 是否允许（建议允许，格式不对时报错并忽略该行）。
- 删掉版本树会连历史一起删；如果用户只想清理构建区，不受影响。

### 10.2 安全更新的判定边界

只按 patch 号，还是叠加 CVE / 上游安全公告？前者实现简单、可预测；后者需要额外的
数据源。默认按 patch 号，CVE 只作为输出里的一列提示。

### 10.3 上游元数据缓存

缓存目录、TTL、失败重试次数、GitHub 限流后的退避策略，需要在 M1 定下来并写进
`docs/RECIPE.md`。

### 10.4 选择器的依赖策略

- 选中扩展时，依赖是「自动勾选」还是「只提示」？
- 取消一个被其他扩展依赖的库时，是阻止、警告、还是自动保留？
- builtin 无法真正关闭时的呈现方式。

### 10.5 buildroot 的实现选型

已定：chroot + dnf rootfs，rootfs 在项目 `.phprt/buildroots/`；
默认绑定项目 + phprt 仓库，自动构建额外绑定 `BUILD_ROOT`。实现见第 8 节和
[CHROOT-BUILD.md](CHROOT-BUILD.md)。

## 11. 非目标

- 不做二进制分发（仍是源码编译）。
- 不接管系统的 PHP、不写系统目录。
- 不在 driver 里增加包专属逻辑；新增能力优先进 recipe 字段或公共函数。
- 不为了 UI 引入常驻服务或数据库。
