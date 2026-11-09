# phprt recipe 规范

本文约束 `bundles/<版本>/{builtin,ext,deps,tools}/*.sh` 的写法。

目标只有一个：**recipe 是数据，driver 是执行器。** 加一个扩展或依赖时，只改 recipe 文件，
不改 `php.sh`、`build.sh`、`common.sh`、`verify`；`bundle.conf` 只保留全局策略和可选覆盖。

规范只约束「driver 需要知道的东西」，不约束编译细节。`configure` 参数在当前 PHP 版本是否
存在、依赖是否正确，由构建日志、`phprt verify` 和开发用的 `scripts/recipe-check.sh --deep`
负责，不在本文范围内。

本文第 11 节是对现有 110 个 recipe 的审计结果；第 12 节记录审计后对规范做的修订。

## 1. 类型与目录

| 类型 | 目录 | 作用 | 产物 |
| --- | --- | --- | --- |
| builtin | `builtin/` | 编进 PHP 本体的扩展，向 `./configure` 提供参数 | PHP 二进制或 `extensions/*.so` |
| ext | `ext/` | 用 `phpize` 单独编译的 PECL 扩展 | `extensions/<name>.so` + ini |
| dep | `deps/` | PHP 或扩展链接的 C 库 | `deps/<name>-<版本>/lib` 或 `lib64` |
| tool | `tools/` | 构建工具（autoconf、re2c、perl 核心模块） | `tools/<name>-<版本>` |

`kind` 和 `name` 默认从路径推断：`builtin/curl.sh` → `kind=builtin name=curl`。头里写出来
只用于自检，写错会报错。

`curl` 同时存在 `builtin/curl.sh` 和 `deps/curl.sh`，这是合法的：前者是 PHP 扩展，后者是
libcurl 库。注册表必须按类型区分，裸名 `curl` 在 `needs` 里只指向 dep/tool，不会指回 builtin。

## 2. 文件结构（v2）

每个 v2 recipe 必须按下面的顺序：

```bash
#!/usr/bin/env bash
# phprt: ...（见第 3 节）
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags()  { ... }   # builtin 必填
needs()  { ... }   # builtin 必填
build()  { ... }   # ext / dep / tool 必填
enable() { ... }   # ext 在 ini != - 时必填
verify() { ... }   # ext 可选

recipe_main "$@"
```

规则：

1. **最后一行必须是 `recipe_main "$@"`**。没有它，脚本只能是 legacy。
2. **不允许顶层构建代码**。`need_cmds`、下载、`phpize`、`make`、写 ini 都必须在函数里。
   顶层只允许：shebang、头注释、`set`、`source`、变量赋值、函数定义。
3. 直接执行脚本时也必须先校验：`bash ext/redis.sh --version` 只输出版本，不触发构建。
4. `contract=legacy` 的文件可以暂时保持顶层写法，但必须登记在
   `bundles/<版本>/legacy-recipe.list` 里；新文件不允许 legacy。

## 3. 头部字段

头部是**一行** `key=value`，放在 shebang 后 10 行内：

```bash
# phprt: contract=v2 kind=ext name=redis switch=ENABLE_REDIS version=REDIS_VERSION modules=redis ini=20-redis.ini tools=autoconf,perlshim needs= optional=
```

### 3.1 字段矩阵

| 字段 | builtin | ext | dep | tool | 含义 |
| --- | --- | --- | --- | --- | --- |
| `contract` | 必填 | 必填 | 必填 | 必填 | `v2` 或 `legacy` |
| `kind` | 可选 | 可选 | 可选 | 可选 | 路径推断，写则必须一致 |
| `name` | 可选 | 可选 | 可选 | 可选 | 路径推断，写则必须一致 |
| `switch` | 必填 | 必填 | 禁止 | 禁止 | 选择开关变量名 |
| `version` | 禁止 | 必填 | 必填 | 必填 | 版本变量名，不写具体版本 |
| `modules` | 必填 | 可选 | 禁止 | 禁止 | 产出的 PHP 模块，逗号分隔；ext 默认 name |
| `type` | 必填 | 必填 | 禁止 | 禁止 | 支持 `static`、`shared`、`static,shared` |
| `default` | 必填 | 必填 | 禁止 | 禁止 | `install` 的默认方式，必须是 `type` 的子集 |
| `ini` | 必填 | 必填 | 禁止 | 禁止 | `-` 或 `20-<name>.ini`；`-` 表示不需要 ini |
| `tools` | 禁止 | 必填 | 必填 | 必填 | 构建工具，可以为空 |
| `needs` | 禁止 | 必填 | 必填 | 禁止 | 硬依赖，可以为空 |
| `optional` | 禁止 | 可选 | 必填 | 禁止 | 条件依赖，可以为空 |
| `wheel` | 禁止 | 禁止 | 可选 | 禁止 | wheel 化例外，默认 `auto`；见第 6 节 |
| `upstream` | 如需检查更新 | 如需检查更新 | 必填（可为 `none`） | 必填（可为 `none`） | 上游来源：`github:owner/repo`、`pecl:包名`、`phpnet:x.y`、`none`（不检查）；也可改用 `versions()` |
| `desc` | 可选 | 可选 | 可选 | 可选 | 一行说明，选择器显示，不参与构建 |

说明：

- **`needs`、`tools`、`optional` 即使为空也必须写。** 这是为了强制作者/AI 明确回答依赖
  问题，而不是忘写。
- **`ini` 不使用默认值。** 审计发现 6 个 ext 里有 5 个的 ini 不是 `20-<name>.ini`
  （`23-protobuf`、`25-apcu`、`26-yaml`、`27-swoole`、`30-xdebug`），默认值会写错文件。
- **`ini` 不再表达编译方式。** 编译方式由 `type` / `default` 决定；`ini=-` 只表示
  static 不需要 ini，shared 由 driver 生成 `20-<name>.ini` 或调用 recipe 的 `enable()`。
- **`type` / `default` 是必填的。** 例：`tidy type=static,shared default=static`；
  `xdebug type=shared default=shared`。`default` 不在 `type` 里会直接报错。
- **static PECL 的构建。** recipe 需要实现 `stage_static()`，调用
  `ext_stage_static <名字> <版本> <URL> <SHA256> [configure 参数...]` 把源码放进
  PHP 源码树的 `ext/<名字>`；外部库用 `static_ldflags()` 补进 PHP 链接行。
- **shared builtin 的构建。** driver 从 PHP 源码树复制 `ext/<名字>` 后单独 `phpize`，
  configure 参数复用 `flags()`（driver 会临时把 `ENABLE_*` 置 1）。
- **`switch` 不要求等于 `ENABLE_<NAME>`。** 例如 `mbstring` → `ENABLE_ONIGURUMA`，
  `zip` → `ENABLE_LIBZIP`。校验只要求变量名大写且确实在脚本里使用。
- **`version` 必须显式写变量名。** 不能按 name 猜：`curl` → `LIBCURL_VERSION`，
  `yaml` → `YAML_EXT_VERSION`，`libsodium` → `LIBSODIUM_VERSION`。
- **dep 禁止 `switch`。** 条件依赖由消费者的 `needs`/`optional` 表达，不在 dep 自己身上。
- **`wheel` 只对 dep 开放。** SONAME 是编译产物，不在 recipe 里描述；这个字段只表达
  例外（静态库、`dlopen` 别名、singleton 组），见第 6 节。
- **`modules` 的多模块情况必须写全**：`libxml2` → `dom,xml,simplexml,xmlreader,xmlwriter,soap`，
  `sqlite` → `sqlite3,pdo_sqlite`，`mysqli` → `mysqli,pdo_mysql`。ext 单模块可省略，默认 name。
- `version` 变量在兼容期可以定义在 `bundle.conf`，目标是在 recipe 内给默认值、允许
  `bundle.conf` 覆盖。driver 通过 `recipe_main --version` 取值，不直接读 bundle.conf。
- **同名解析分两套。** 扩展名（builtin / PECL）用 `ext_recipe_sh`：builtin → ext；
  依赖名（`needs` / `tools`）用 `module_sh`：deps → ext → tools → builtin。
  `libxml2`、`sqlite`、`zlib`、`curl`、`openssl` 都有同名的 builtin 和 dep，
  两套解析不能混用。

### 3.2 兼容别名

迁移期允许旧头：

| 旧写法 | 等价于 |
| --- | --- |
| `builtin=curl switch=ENABLE_LIBCURL` | `kind=builtin name=curl switch=ENABLE_LIBCURL` |
| `switch=ENABLE_REDIS` | `kind=ext name=redis switch=ENABLE_REDIS`（按目录推断） |
| `needs=zlib,libpng` | `needs=zlib,libpng` |

旧头没有 `contract` 的，一律按 `legacy` 处理；必须在 `legacy-recipe.list` 里。

## 4. 函数契约

### builtin

```bash
flags() {
  if [ "${ENABLE_LIBCURL:-0}" = 1 ]; then
    printf '%s\n' "--with-curl=$(dep_dir_of curl)"
  else
    printf '%s\n' --without-curl
  fi
}

needs() {
  [ "${ENABLE_LIBCURL:-0}" = 1 ] && printf '%s\n' curl
}
```

- `flags`：输出 PHP `./configure` 参数，一行一个；关闭时必须输出负向参数（`--without-*` 或
  `--disable-*`），除非该参数在当前 PHP 版本已不存在。
- `needs`：输出这个扩展启用的依赖名；关闭时输出空。builtin 的依赖是**动态**的，所以不写
  头字段 `needs`。
- 两个函数都必须是纯查询：不下载、不建目录、不写文件。

### ext

```bash
build()  { ... }   # 下载、phpize、configure、make、make install
enable() { ... }   # 只写 ini；ini != - 时必填
verify() { ... }   # 可选
```

`build()` 只负责编译和安装 `.so`，**不写 ini**。`enable()` 由 driver 在启用阶段调用，只写
`$PREFIX/etc/conf.d/<ini>`。带自定义配置的扩展必须实现 `enable()`：

```ini
; xdebug
zend_extension=xdebug.so
xdebug.mode=off
```

### dep / tool

```bash
build() { ... }    # 下载、configure、make、make install
```

dep/tool 不写 ini，不碰 PHP 配置。tool 可以依赖另一个 tool，用 `tools=perlshim` 表达。

### 静态链接（`--static-all`）

普通安装只要求 dep 提供共享库；`--static-all` 还要求提供 `.a`。recipe 的义务：

1. 静态构建时把 `.a` 装到 `deps/<名字>-<版本>/lib/` 或 `lib64/` 根目录。
2. 同时保留 `.so` 不冲突：PHP 静态链接时只用 `.a`，普通安装仍可用 `.so`。
3. 不要求为静态库单独改安装目录名；`.a` 和 `.so` 可以并存在同一个 `lib/`。
4. 依赖的私有链接参数（例如 C++ 库的 `-lstdc++`、ICU 的 `-licuuc -licudata`）
   由 driver 的静态链接累计；recipe 只有在 `pkg-config --static` 也不完整时才
   需要额外声明。
5. `wheel=none` 的 dep 可以只提供 `.a`；wheel 化不会因此失败。

静态模式下 driver 会：

- 依赖构建参数里去掉绝对 rpath，只保留 `-L`；
- 把 PHP 的 `EXTRA_LIBS` 包进 `-Wl,-Bstatic --start-group … --end-group -Wl,-Bdynamic`；
- 把 `static_ldflags()` 的输出放到链接组末尾；
- 加 `-static-libgcc`，有 C++ 依赖时加 `-static-libstdc++`。

`static_ldflags()` 的契约（ext recipe 可选）：

```bash
static_ldflags() {
  printf '%s\n' -lssh2
}
```

- 输出一行一个链接参数，只用于 `--static` / `--static-all` 的 PHP 链接；
- 不要在里写 `-L` 或 rpath，依赖路径由 driver 统一加；
- 外部库必须已经在 `build()` / `stage_static()` 阶段构建；
- 普通 shared 构建不会调用它。

v0 的支持范围是 `openssl zlib curl sqlite mbstring`；完整计划见
[STATIC-ALL.md](STATIC-ALL.md)。

### 版本查询（可选，供 `phprt outdated` / `phprt update` 用）

两种写法，按来源复杂度选：

1. **头部 `upstream=`**（常见来源，driver 直接查）：

   | 取值 | 实现 | 例子 |
   | --- | --- | --- |
   | `github:owner/repo` | GitHub tags，取其版本号 | `github:openssl/openssl` |
   | `pecl:包名` | PECL 的稳定版列表 | `pecl:swoole` |
   | `phpnet:系列` | php.net 某系列的最新版 | `phpnet:8.5` |

2. **函数 `versions()`**（特殊来源，在 recipe 里写）：

   ```bash
   versions() {
     upstream_versions_listing https://ftp.gnu.org/gnu/autoconf/ \
       'autoconf-[0-9]+(\.[0-9]+)+\.tar\.xz'
   }
   ```

   - 输出一行一个稳定版版本号；预发布版（alpha/beta/rc/dev）不要输出。
   - 可以用 `runner/upstream.sh` 里的公共函数：`upstream_versions_listing`、
     `upstream_versions_github`、`upstream_versions_pecl`、`upstream_versions_phpnet`。
   - 查询失败返回非 0；driver 会把它标记为「查不到」，不会中断安装。

约束：

- 版本查询是**纯查询**：不下载、不解压、不写构建目录；结果缓存在
  `~/.cache/phprt/upstream/`（默认 6 小时）。
- 代理顺序：`https_proxy` / `HTTPS_PROXY` → `http_proxy` / `HTTP_PROXY` → 直连 →
  `PHPRT_PROXY`。只设 `http_proxy` 时，HTTPS 请求也用它。
- `OFFLINE=1` 时不发起请求。
- 下载地址默认由已有的 `*_URL` 模板替换版本号得到；URL 规则特殊的再实现 `download_url <版本>`。

## 5. 输出与副作用

- `recipe_main --version` / `--flags` / `--needs` 的 stdout **只能是数据**，一行一个；日志走
  stderr（`log` 已经写 stderr）。
- 查询动词 2 秒内必须返回，不能触发下载和编译。
- 构建动词可以输出日志，但不能改写其他 recipe 的产物。
- 所有写文件操作必须落在 `$TREE`、`$DEPS`、`$PHPRT_HOME` 之内；不允许写系统目录。

## 6. wheel 化与 SONAME

运行时库整理（wheel 化）由 driver 在构建收尾和 `phprt update` 时统一做一次，算法见
[WHEEL-LIBS.md](WHEEL-LIBS.md)。recipe 里**不写** SONAME、不写运行时名、不写 rpath。

### 6.1 为什么 SONAME 不进 recipe

- SONAME 是编译产物，只有 `make install` 之后才存在。recipe 里写死预期值，等于把上游的
  事实抄一遍，升级就漂移。
- wheel 化的输入是 `deps/*/lib{,64}/*.so*` 的 ELF 头。它要回答的是「整棵树里谁引用了
  谁」，单个 recipe 看不到二三级依赖。
- 消费方（`bin/php`、各扩展 `.so`）可能引用 recipe 根本不知道的库，只有扫描 ELF 才能
  全部改写。

所以默认情况下，recipe 对 wheel 化的义务只有「按标准方式安装」；只有例外才需要声明。

### 6.2 recipe 必须保证的事

1. 共享库装在 `deps/<name>-<版本>/lib/` 或 `lib64/` 的**根目录**。子目录
   （`lib/engines-3/`、`lib/ossl-modules/`）不会被扫描。
2. 真实文件名以 `.so.` 开头（`libfoo.so.N`，完整版 `libfoo.so.N.M.P` 也算），
   `DT_SONAME` 带 ABI 版本号（`libfoo.so.N`）。缺 SONAME 或 SONAME 没有版本号，wheel 化
   直接失败（`SONAME 没有 ABI 版本号，无法生成运行时名`）。
3. `.so`、`.so.N` 这类软链可以保留。wheel 化只取真实文件；不要删上游软链，后续
   `add ext` 链接还要用。
4. 只产静态库（`.a`）或只装头文件的依赖写 `wheel=none`。wheel 化本来就会跳过没有 `.so`
   的 dep，声明的作用是让校验能断言「这个 dep 不该有共享库」，而不是漏装。
5. 不要在 `build()` 里 `patchelf`、不要手工改 `DT_NEEDED`、不要把 `deps/*/lib/*.so`
   软链到 `lib/phprt`（原因见 WHEEL-LIBS 6.1、6.2）。
6. 构建期可以写 rpath（CMake 的 `CMAKE_INSTALL_RPATH`、`-Wl,-rpath`），它只对 `deps/`
   里的中间产物有效；运行时产物由 wheel 化统一覆盖。不要往 `lib/phprt` 或扩展里写
   绝对路径。
7. 安装目录名必须是 `<name>-<版本>`：`wheel.map` 的版本列取自目录名，`dep_dir_of`
   必须给出同样的名字。
8. 不要在两个 dep 里装同一个 SONAME。同名 SONAME 出现两份时 wheel 化直接失败
   （旧版本目录没清干净是常见原因，`prune_stale_deps` 负责清理）。

### 6.3 例外声明 `wheel=`

只有 dep 需要这个字段，可省略（等于 `wheel=auto`）：

```bash
# phprt: contract=v2 kind=dep name=openssl version=OPENSSL_VERSION needs= optional= tools=perlshim wheel=singleton:libssl+libcrypto
```

| 值 | 含义 |
| --- | --- |
| `auto` | 默认：所有 `.so.N` 拍平 |
| `none` | 不参与 wheel 化（只产 `.a`、可执行文件或数据） |
| `exclude:a+b` | 装出来但不复制到 `lib/phprt`（已知没有消费者） |
| `alias:libzip.so.5` | 代码里有 `dlopen("libzip.so.5")`，在 `lib/phprt` 补一个指向 rt 名的软链 |
| `singleton:libssl+libcrypto` | 组内每个 SONAME 恰好一份，且 map 里的版本列一致 |

多条指令用 `;` 连接，指令内的名字用 `+` 分隔：

```bash
wheel=singleton:libssl+libcrypto;exclude:libicutest+libicutu
```

`singleton` 用来替代校验代码里写死的 `libssl.so.3`；跨 ABI 系列（`libssl.so.4`）时不用
改校验逻辑。

### 6.4 校验里的 wheel 项

| 层 | 检查 |
| --- | --- |
| L1 | `wheel` 只允许出现在 dep；值只能是 `auto`、`none` 或上表的指令；`alias:` 的值必须是 `*.so.*`；`exclude`/`singleton` 的值不能带路径 |
| L2 | 构建后扫描 `deps/<name>-<版本>/lib{,64}/*.so.*`：每个真实文件必须有带 ABI 版本号的 SONAME；`wheel=none` 的 dep 不得产出 `.so.*`；`exclude` 的库不得出现在任何消费方的 `DT_NEEDED` 里；`alias` 的名字必须在 `lib/phprt` 能解析到 |
| L3 | `phprt verify` 的 wheel 完整性检查遍历 `wheel.map` 加所有 `singleton`/`alias` 声明，不再写死 SONAME |

L2 的「消费方」指 `bin/*`、`lib/php/extensions/*/*.so` 和 `lib/phprt/*.so*`。

### 6.5 现状（PHP 8.5.12）

- `wheel.map` 27 行；`bin/php` 和所有扩展的 `DT_NEEDED` 全是 rt 名，没有原始 SONAME 残留。
- `libffi` 只装到 `lib64/`，靠 glob 覆盖，不需要额外声明。
- OpenSSL 的 `lib/engines-3/*.so`、`lib/ossl-modules/legacy.so` 不在 `lib/` 根目录，没有
  被拍平。`OPENSSLDIR` 是 `/etc/pki/tls`，这些模块本来也只从系统目录加载；树里要用
  legacy provider 需要显式设 `OPENSSL_MODULES`，不靠 wheel。这是已知边界。
- 4 个库被拍平但没有消费方 `NEEDED`：`libicutest`、`libwebpdecoder`、`libwebpdemux`、
  `libwebpmux`。可以用 `exclude` 去掉，体积都不大，不做也不算错。
- 目前没有 dep 需要 `alias`，也没有 SONAME 缺版本号的情况。

## 7. 校验

校验分三层，避免每次构建都做重检查。

| 层 | 内容 | 触发点 | 成本 |
| --- | --- | --- | --- |
| L1 静态 | 头存在且唯一、字段矩阵、name/kind 一致、switch 被引用、version 变量存在、ini 格式、函数存在、最后一行 `recipe_main`、顶层无构建代码 | `module_sh`、`recipe_main` | 毫秒级，按文件 sha 缓存 |
| L2 动态 | `--version` 非空、`--meta` 2 秒内返回且 stdout 纯 `key=value`、enabled 时 `--flags` 非空、`--needs` 可解析、`enable()` 写出的 ini 与声明一致 | `--plan`、签名计算、`verify` | 每 recipe 一次进程，按构建会话缓存 |
| L3 显式 | 遍历所有 recipe 跑 L1+L2；`--deep` 用 `configure --help` 检查参数 | `scripts/recipe-check.sh`、CI | 秒级 |

L1 必须检查的字段规则：

- `contract` 必须存在；`legacy` 必须在 allowlist 里。
- `switch`：builtin/ext 必填，dep/tool 禁止；必须匹配 `^ENABLE_[A-Z0-9_]+$`，且在脚本中出现。
- `version`：ext/dep/tool 必填；变量必须在 recipe 或 `bundle.conf` 中定义。
- `modules`：builtin 必填且非空；ext 可省略，省略时等于 name。
- `ini`：builtin/ext 必填；值必须是 `-` 或 `^[0-9]{2}-[a-z0-9_-]+\.ini$`。
- `tools` / `needs` / `optional`：键必须存在；每个值必须能解析到对应的 dep/tool。
- 函数：builtin 需要 `flags`/`needs`；ext/dep/tool 需要 `build`；ext 在 `ini != -` 时需要
  `enable`。
- 最后一行必须是 `recipe_main "$@"`；顶层不允许出现 `need_cmds`/`fetch`/`phpize`/`make`/
  `configure`/写 ini 等命令。

错误格式固定，方便 AI 照补：

```text
错误: bundles/php8.5/ext/redis.sh 缺少必填字段: version, ini
规范: docs/RECIPE.md#3-头部字段
```

### legacy 兼容

现有 110 个 recipe 全部是 legacy（没有 `contract`，没有 `recipe_main`）。迁移期：

- 新文件必须是 `contract=v2`；没有 `contract=v2` 且不在
  `bundles/<版本>/legacy-recipe.list` 里的文件直接报错。
- legacy 文件跑 L1 头检查 + L2 查询检查，不强制函数结构；构建时打印一次
  `warn "legacy recipe: ... 请迁移到 v2"`。
- allowlist 只减不增；迁移一个删一个。

## 8. 模板

### builtin

```bash
#!/usr/bin/env bash
# phprt: contract=v2 kind=builtin name=curl switch=ENABLE_LIBCURL modules=curl ini=-
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

flags() {
  if [ "${ENABLE_LIBCURL:-0}" = 1 ]; then
    printf '%s\n' "--with-curl=$(dep_dir_of curl)"
  else
    printf '%s\n' --without-curl
  fi
}

needs() {
  [ "${ENABLE_LIBCURL:-0}" = 1 ] && printf '%s\n' curl
}

recipe_main "$@"
```

builtin 的 `ini` 为 `-` 时不需要 `enable()`；为 `20-zlib.ini` 这类共享模块时，driver 按
`modules` 生成 `extension=<module>.so` 行。

### ext

PECL 扩展用 `ext_pecl_build`（`runner/recipe-ext.sh`），recipe 只写差异：

```bash
#!/usr/bin/env bash
# phprt: contract=v2 kind=ext name=redis switch=ENABLE_REDIS version=REDIS_VERSION modules=redis ini=20-redis.ini tools=autoconf,perlshim needs= optional= upstream=pecl:redis
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"

build() {
  ext_pecl_build redis "$REDIS_VERSION" "$REDIS_URL" "$REDIS_SHA256" \
    --enable-redis --enable-redis-session --enable-redis-json
}

enable() {
  mkdir -p "$PREFIX/etc/conf.d"
  cat > "$PREFIX/etc/conf.d/20-redis.ini" <<INI
; 由 phprt 生成（redis $REDIS_VERSION）
extension=redis.so
INI
}

recipe_main "$@"
```

`ext_pecl_build <名字> <版本> <URL> <SHA256> [configure 参数...]` 负责：下载、按 PHP
版本分源码目录解压、autoconf/perlshim 环境、phpize、configure、make、install、
检查 `.so`、写 stamp。需要额外处理时用钩子：

| 钩子 | 用途 |
| --- | --- |
| `ext_prepare()` | 解压后、phpize 前执行，cwd 是源码目录（打补丁） |
| `EXT_MODULE` | 产物模块名，默认等于 `<名字>`（多模块扩展用） |
| `EXT_SIG_EXTRA` | 追加到内层 stamp 的签名内容（一般不需要） |
| `enable()` | 仍然由 recipe 写，因为 ini 内容各不相同 |

需要额外库时在 `build()` 里先 `dep_env <名字>`，再把库路径作为 configure 参数传入。
`apcu`（22 行）、`igbinary`（31 行）、`redis`、`xdebug`、`yaml` 都是现成例子。

### dep

```bash
#!/usr/bin/env bash
# phprt: contract=v2 kind=dep name=curl version=LIBCURL_VERSION needs= optional=openssl,zlib tools= wheel=auto
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

build() {
  # 下载、configure、make、make install
}

recipe_main "$@"
```

### tool

```bash
#!/usr/bin/env bash
# phprt: contract=v2 kind=tool name=autoconf version=AUTOCONF_VERSION tools=perlshim
set -euo pipefail
source "${PHPRT_RUNNER:?}/common.sh"
load_config

build() { ... }

recipe_main "$@"
```

## 9. 迁移

### 9.1 阶段

1. **Phase A：补头。** 给所有 recipe 加 `contract=legacy` 和缺失字段，登记 allowlist。此阶段
   L1 头校验立即生效，能挡住新文件漏字段。
2. **Phase B：包函数。** 按 ext → builtin → dep → tool 的顺序把顶层代码包进 `build()` /
   `flags()` / `needs()` / `enable()`，最后一行加 `recipe_main "$@"`，改成 `contract=v2`，
   从 allowlist 删除。ext 优先，因为 ini/tools/version 的问题最集中。
3. **Phase C：版本下沉。** 把 `*_VERSION` / `*_URL` / `*_SHA256` 从 `bundle.conf` 移到 recipe
   默认值，`bundle.conf` 只做覆盖。
4. **Phase D：去掉硬编码表。** `BUILTIN_ORDER`（`php.sh` + `build.sh`）、`ext_modules`、
   `ext_version`、`dep_sig` 里的 `case curl|libxml2|libzip` 都改为读 recipe 的
   `modules`/`version`/`optional`；`wheel_integrity_check` 和 `update` 里写死的
   `libssl.so.3` / `libcrypto-rt3.so.3` 改为读 dep 的 `wheel=singleton:...`。

### 9.2 顺序问题

依赖图目前是列表，不是拓扑序；`--ext` 的输入顺序仍可能影响构建顺序。审计已确认
`deps/curl.sh` 的 openssl/zlib 边没有声明，`--ext "curl openssl"` 会先编 libcurl。
Phase D 之后应在 resolver 里做拓扑排序；本文不做完整图模型，只要求 `needs`/`optional`
声明完整，顺序由 driver 统一排序。

## 10. 提交前检查

**添加或修改扩展/依赖后，必须运行契约检查，通过才算添加成功：**

```bash
scripts/recipe-check.sh          # 静态检查，不联网
scripts/recipe-check.sh --deep    # 额外确认上游可查、pin 在版本列表里
```

这是开发工具，只在仓库里跑，不通过 `phprt` 暴露给用户。

- [ ] 新文件从模板复制，`contract=v2`
- [ ] 头部字段齐全，字段矩阵里的禁止项没有出现
- [ ] `name` 与文件名一致（如果写了）
- [ ] `switch` 已定义且在脚本中使用
- [ ] `version` 变量已定义，`recipe_main --version` 有输出
- [ ] 声明了 `upstream=`（或 `upstream=none`），或实现了 `versions()` / `latest_version()`
- [ ] `upstream=` 格式合法，且能在上游查到
- [ ] `desc`（可选）不包含空格
- [ ] `modules` 多模块写全；`ini` 与实际文件名一致
- [ ] `needs=` / `tools=` / `optional=` 已显式声明（即使为空）
- [ ] `build()` 不写 ini，`enable()` 只写 ini
- [ ] `needs`/`tools` 里的每个名字都能解析
- [ ] 关闭时 `flags` 输出负向参数，或说明当前 PHP 版本已移除该参数
- [ ] dep 的共享库装在 `lib/` 或 `lib64/` 根目录，真实文件是 `.so.N` 且 SONAME 带 ABI 版本号
- [ ] recipe 里没有手工 `patchelf`、没有把 `deps/*/lib/*.so` 软链到 `lib/phprt`
- [ ] 只产静态库或需要 `dlopen` 别名时，已写 `wheel=none` / `wheel=alias:...`
- [ ] `phprt install <版本> --plan` 能看到新 recipe
- [ ] `scripts/recipe-check.sh` 通过；联网环境再跑一次 `--deep`

## 11. 审计结果（2026-11-08）

范围：`php8.5` / `php8.6` / `php7.4` 三个 bundle，共 110 个 recipe。

统计基线是提交 `ee8cd5b`。之后新增的 recipe（`gettext` builtin、`xsl`/`libxslt` 等）没有计入
本节计数，但适用同一规范。

审计方法：遍历三个 bundle 的 `{builtin,ext,deps,tools}/*.sh`，解析 `phprt` 头、函数定义、
顶层命令、ini 写入、版本变量引用，并把 `needs`/`tools` 解析到实际文件；同时核对
`common.sh` 里的 `ext_modules`/`ext_version` 和 `bundle.conf` 的版本变量。

### 11.1 汇总

| bundle | builtin | ext | dep | tool | 合计 |
| --- | --- | --- | --- | --- | --- |
| php8.5 | 12 | 6 | 17 | 3 | 38 |
| php8.6 | 11 | 6 | 16 | 3 | 36 |
| php7.4 | 11 | 6 | 16 | 3 | 36 |

合规情况：

| 项 | 结果 |
| --- | --- |
| 有 `contract=v2` | 0 / 110 |
| 有 `recipe_main` | 0 / 110 |
| builtin 有 `modules`/`ini` | 0 / 34 |
| ext 有 `version`/`tools` | 0 / 18 |
| dep 有完整头 | 0 / 49；16 / 49 有部分头（`freetype`、`libpng`、`libzip`、`libsodium`、`avif`、`webp` 等，缺 `contract`/`version`/`optional`） |
| tool 有头 | 0 / 9 |

结论：规范不能直接切 error 模式，必须先走 Phase A/allowlist；否则现有构建全部失败。

### 11.2 builtin（php8.5）

| recipe | switch | modules | ini |
| --- | --- | --- | --- |
| curl | ENABLE_LIBCURL | curl | - |
| ffi | ENABLE_FFI | ffi | - |
| gd | ENABLE_GD | gd | - |
| intl | ENABLE_INTL | intl | - |
| libxml2 | ENABLE_LIBXML2 | dom,xml,simplexml,xmlreader,xmlwriter,soap | - |
| mbstring | ENABLE_ONIGURUMA | mbstring | - |
| mysqli | ENABLE_MYSQLI | mysqli,pdo_mysql | - |
| openssl | ENABLE_OPENSSL | openssl | - |
| sodium | ENABLE_SODIUM | sodium | - |
| sqlite | ENABLE_SQLITE | sqlite3,pdo_sqlite | 20-sqlite.ini |
| zip | ENABLE_LIBZIP | zip | - |
| zlib | ENABLE_ZLIB | zlib | 20-zlib.ini |

审计发现：

- `libxml2`、`sqlite`、`mysqli` 是多模块，`modules` 必须写全，不能默认 name。
- `zlib`、`sqlite` 是共享模块，`ini` 不是 `-`；其余 builtin 静态编入。
- `mbstring` 的开关叫 `ENABLE_ONIGURUMA`，`zip` 叫 `ENABLE_LIBZIP`，不能按 name 推断。

### 11.3 ext（php8.5）

| recipe | switch | version | modules | ini | tools | needs |
| --- | --- | --- | --- | --- | --- | --- |
| apcu | ENABLE_APCU | APCU_VERSION | apcu | 25-apcu.ini | autoconf,perlshim | |
| protobuf | ENABLE_PROTOBUF | PROTOBUF_VERSION | protobuf | 23-protobuf.ini | autoconf,perlshim | |
| redis | ENABLE_REDIS | REDIS_VERSION | redis | 20-redis.ini | autoconf,perlshim | |
| swoole | ENABLE_SWOOLE | SWOOLE_VERSION | swoole | 27-swoole.ini | autoconf,perlshim | openssl,zlib |
| xdebug | ENABLE_XDEBUG | XDEBUG_VERSION | xdebug | 30-xdebug.ini | autoconf,perlshim | zlib |
| yaml | ENABLE_YAML | YAML_EXT_VERSION | yaml | 26-yaml.ini | autoconf,perlshim | libyaml |

审计发现：

- 6/6 都在脚本体内写 ini（`conf.d/*.ini`），且 ini 文件名只有 redis 是 `20-<name>.ini`，
  其余是 23/25/26/27/30。**`ini` 必须显式，不能默认。**
- 6/6 都调用 `module_sh autoconf` 和 `module_sh perlshim`，但 0/6 声明 `tools`。
- 6/6 都没有 `version` 字段；`ext_version()` 里硬编码了 6 个变量名。
- 6/6 没有 `recipe_main`，顶层直接构建；直接问版本会触发构建。
- `yaml` 的版本变量是 `YAML_EXT_VERSION`，不是 `YAML_VERSION`。

### 11.4 dep（php8.5）

| recipe | version | needs | optional | tools |
| --- | --- | --- | --- | --- |
| aom | AOM_VERSION | | | |
| avif | AVIF_VERSION | aom | | |
| curl | LIBCURL_VERSION | | openssl,zlib | |
| freetype | FREETYPE_VERSION | zlib,libpng | | |
| icu | ICU_VERSION | | | |
| jpeg | JPEG_VERSION | | | |
| libffi | LIBFFI_VERSION | | | |
| libpng | LIBPNG_VERSION | zlib | | |
| libsodium | LIBSODIUM_VERSION | | | |
| libxml2 | LIBXML2_VERSION | | zlib | |
| libyaml | LIBYAML_VERSION | | | |
| libzip | LIBZIP_VERSION | openssl,zlib | | |
| oniguruma | ONIGURUMA_VERSION | | | |
| openssl | OPENSSL_VERSION | | | perlshim |
| sqlite | SQLITE_VERSION | | | |
| webp | WEBP_VERSION | | | |
| zlib | ZLIB_VERSION | | | |

审计发现：

- 11/17 完全没有 `phprt` 头（aom、curl、icu、jpeg、libffi、libxml2、libyaml、oniguruma、
  openssl、sqlite、zlib）。
- `curl`、`libxml2` 使用条件 openssl/zlib，但头里没有 `optional`；`dep_sig()` 里用
  `case curl|libxml2|libzip` 硬编码。
- `avif`、`webp` 头里有 `switch=ENABLE_GD_AVIF` / `ENABLE_GD_WEBP`。dep 不应该有 switch；
  条件已经由 `gd --needs` 表达。
- `openssl` 使用 `dep_dir_of perl5core` 并在体内补 perl 模块，但头里没有 `tools=perlshim`。
- 17/17 顶层直接构建，没有 `recipe_main`。

### 11.5 tool（php8.5）

| recipe | version | tools |
| --- | --- | --- |
| autoconf | AUTOCONF_VERSION | perlshim |
| perlshim | PERL_SHIM_VERSION | |
| re2c | RE2C_VERSION | |

审计发现：

- 3/3 没有头。
- `autoconf` 会调用 `tools/perlshim.sh`，必须声明 `tools=perlshim`；否则 resolver 不会先装
  perl 模块。
- `deps/openssl.sh` 也依赖 perlshim，说明 tool 依赖不只出现在 tool 之间。

### 11.6 php7.4 / php8.6 差异

结构与 8.5 相同，差异只在版本和 recipe 集合：

- 8.5 有 `sodium` builtin + `libsodium` dep；8.6 / 7.4 没有，所以各少 2 个 recipe。
- 7.4 的 PHP 参数不同（`--enable-opcache`、`--with-icu-dir` 仍有效，libxml2 是 2.9.14），
  这些属于 recipe 内部逻辑，不影响本文的结构规范。
- 8.6 是 dev 分支，PHP 源码需要 `buildconf`，`php.sh` 的 tool 依赖是条件性的；这是 PHP 本体
  特例，不走 recipe 注册表。

### 11.7 wheel 化与 SONAME

审计对象是构建产物（PHP 8.5.12，`lib/phprt/wheel.map` 27 行），不是 recipe 字段：

| 项 | 结果 |
| --- | --- |
| 消费方残留原始 SONAME | 0（`bin/php`、9 个扩展 `.so` 全是 rt 名） |
| `lib/phprt` 里有文件不在 `wheel.map` | 0 |
| SONAME 缺 ABI 版本号 | 0 |
| 装在 `lib64/` 而非 `lib/` | 1（`libffi`），靠 glob 覆盖 |
| 子目录库未拍平 | OpenSSL 的 `lib/engines-3/*.so`、`lib/ossl-modules/legacy.so` |
| 拍平但无消费者 NEEDED | 4（`libicutest`、`libwebpdecoder`、`libwebpdemux`、`libwebpmux`） |
| 需要 `dlopen` 别名 | 0 |

校验代码里的硬编码：

- `runner/common.sh` 的 `wheel_integrity_check` 写死 `libssl.so.3` / `libcrypto.so.3`；
- `runner/commands.sh` 的 `update` 写死 `libcrypto-rt3.so.3`。

三个 bundle 现在都是 OpenSSL 3.5.9，所以能跑通；换成 OpenSSL 1.1 或 4.x 时这两处会静默
失效。规范里用 dep 的 `wheel=singleton:libssl+libcrypto` 替代，跨 ABI 系列不用改代码。

## 12. 审计后对规范的修订

审计暴露了 v1 草案里几个不现实的默认值，最终版做了这些调整：

1. **`ini` 取消默认值。** v1 想默认 `20-<name>.ini`；实际 6 个 ext 里 5 个不是。现在要求
   builtin/ext 显式写，`-` 表示静态。
2. **`tools` 提升为 ext/dep 必填。** 6/6 ext 都用了 autoconf/perlshim，0/6 声明；openssl
   dep 也用了 perlshim。不强制写就会继续隐藏。
3. **`version` 提升为 ext/dep/tool 必填。** `curl`/`yaml`/`libsodium` 的变量名无法从 name
   推断，`ext_version()` 和 `dep_sig()` 的硬编码必须被替代。
4. **builtin 的 `needs` 改为函数，不写头字段。** builtin 的依赖随开关变化（curl 开才有
   libcurl），静态头表达不了。ext/dep 仍用头字段。
5. **dep 禁止 `switch`。** `avif`/`webp` 现有的 switch 是冗余的，条件由 gd 的 `needs` 表达。
6. **取消 `link` 字段。** 用 `ini=-` / `ini=<文件>` 已经能表达静态/共享，少一个字段。
7. **新增 `contract=legacy` + allowlist。** 审计显示 0/110 有 `recipe_main`，直接要求 v2 会
   让所有构建失败；legacy 让 Phase A 可以先落地头校验。
8. **tool 依赖按节点处理。** `autoconf` → `perlshim` 说明 `tools` 不是平铺列表；resolver
   必须能递归构建 tool，且不能对 tool 调用 `dep_rt_id`（它只适用于 `deps/`）。
9. **`modules` 对 builtin 必填、对 ext 可默认。** builtin 里 3 个是多模块；ext 6 个都是
   单模块且等于 name，默认 name 足够。
10. **`legacy` 只减不增。** 新文件必须是 v2；AI 复制旧文件不会自动获得 legacy 豁免，必须
    显式进 allowlist，而 allowlist 由人维护。
11. **wheel 化不写进 recipe，只写例外。** SONAME 是编译产物，写进头会漂移；新增
    `wheel=` 字段，只表达静态库、`dlopen` 别名、singleton 组和排除列表。
12. **recipe 禁止手工碰 ELF。** 不许 `patchelf`、不许改 `DT_NEEDED`、不许把
    `deps/*/lib/*.so` 软链到 `lib/phprt`；这些都会和 wheel 化冲突。
13. **`singleton` 替代校验里的写死 SONAME。** `libssl.so.3` / `libcrypto-rt3.so.3` 这类
    硬编码应在 Phase D 改为读 dep 的 `wheel=singleton:...`。

## 13. 已知边界

校验能抓「漏写」，抓不了「写错」：

- `optional=` 为空是合法的，curl 少写 `optional=openssl,zlib` 这类语义错误抓不到，只能靠
  `--plan`、构建日志和 `verify`。
- 不校验 `configure` 参数在当前 PHP 版本是否存在；这需要实际跑 configure，归 `--deep`。
- 依赖图目前只做列表，不做拓扑排序；`--ext` 的输入顺序仍可能影响构建顺序，Phase D 后单独修。
- 版本/URL/SHA 完全下沉到 recipe 之前，“加一个扩展只加一个文件”只在 headers + build 层面
  成立；完全成立要等 Phase C。
- wheel 化只扫 `deps/*/lib{,64}/` 根目录，子目录里的库（OpenSSL engines/providers）不拍平，
  运行时也不会自动找到；需要时用 `OPENSSL_MODULES` 等变量显式指定。
- `dlopen("libfoo.so.1")` 这类字符串不会改写，只能靠 `wheel=alias:` 补软链；不声明就会在
  运行时报找不到库。
- `wheel=` 的四个指令里，`none` 和 `exclude` 只需要 driver 少复制；`alias` 和 `singleton`
  目前在代码里还是 OpenSSL 特判，Phase D 才变成通用逻辑。
