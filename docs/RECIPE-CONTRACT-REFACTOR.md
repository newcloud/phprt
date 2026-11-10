# recipe 契约重构方案

> 状态：草案，待评审
>
> 基线：`63ced80 feat: 数据库扩展拆分与分类选择菜单`
>
> 目标文档：`docs/RECIPE.md`（规范）、`docs/EXTENSIONS.md`（进度与台账）
>
> 本文只描述方案，不代表已经实现。

## 0. 实施状态（2026-11-10）

| 阶段 | 状态 | 说明 |
| --- | --- | --- |
| Phase 0 | 已完成 | recipe-check bug、category/menu_order 校验、aliases、文档事实修正 |
| Phase 1 | 已完成 | `ext list --json` 完整契约、`--name`、`needs_detail`、notes |
| Phase 2 | 已完成 | `runner/contract.sh`、唯一字段表、未知/重复/禁止字段 |
| Phase 3 | 已完成 | accessor 收口；消费者侧不再直接读字段 |
| Phase 4 | 已完成 | builtin `needs=`、`notes.conf`、readline/ncurses/posix note |
| Phase 5 | 部分完成 | AGENTS 读取顺序、templates、verify modules；lint/doc-check 待做 |
| Phase 6 | 部分完成 | ext `configure_args()` 已进 JSON；版本/source/opts/wheel 收口待做 |
| Phase 7 | 部分完成 | `tests/contract` fixtures；v3 decoder 待做 |

## 1. 目标

把 recipe 契约从“可执行的 bash + 人读的文档”升级成三层：

1. **机器可读**：`phprt ext list <系列> --json` 一次给出 AI 需要的完整契约。
2. **单点校验**：字段、值域、必填/禁止项只在 `runner/contract.sh` 定义一次。
3. **单页规范**：`docs/RECIPE.md` 只保留 normative 部分，字段矩阵由代码生成。

最终验收标准：

> 加一个扩展或依赖，只改 `bundles/`；改一个契约字段，只改 schema、accessor 和文档。

## 2. 非目标

- 不把头部从 `# phprt: key=value` 换成 YAML/JSON/TOML。
- 不批量重写现有 77 个 recipe。
- 不在本轮重写构建、签名、wheel 化逻辑。
- 不新增 `phprt recipe` 子命令；lint 仍挂在 `scripts/recipe-check.sh`。
- 不改变 `bundle.conf` 的变量命名（Phase C 另议）。

## 3. 现状

### 3.1 数量与事实

基线 `63ced80`：

| 类型 | 数量 |
| --- | ---: |
| builtin | 30 |
| ext（PECL） | 14 |
| dep | 30 |
| tool | 3 |
| 合计 | 77 |

- 44 个 builtin/ext 全部有 `category`。
- 6 个有 `menu_order`。
- 头部没有未知键、重复键、缺失必填项；`menu_order` 都是数字；最后一行都是 `recipe_main "$@"`。
- **14 个 ext 的 `switch` 在 recipe 体内没有被引用**，只被 driver 用来设置 `ENABLE_*`。现在“switch 必须在脚本中使用”的检查是因为命中头部才恒真。

### 3.2 解析与校验现状

| 环节 | 位置 | 问题 |
| --- | --- | --- |
| 头部文本解析 | `common.sh` `recipe_header` / `recipe_field` | 单点，格式变化被关在这里，做得好 |
| 字段读取 | 6 个文件、50 处 `recipe_field` 调用（消费者侧 14 处） | 没有 accessor 的字段改名要全仓 grep |
| 必填/禁止校验 | `common.sh` `recipe_validate` | 180 行 if 链；`req` 矩阵和独立 if 块并存；禁止项一半没实现 |
| 未知/重复字段 | 无 | 静默忽略，契约改名后旧字段不会报错 |
| `opts()` 校验 | `common.sh` + `recipe-check.sh` | 两份几乎逐字相同的 awk |
| `wheel=` 校验 | `common.sh` | 校验和解析分离 |
| 依赖合并 | `module_needs` / `ext_needs` / `source_root_needs` | 三份逻辑 |
| 版本取值 | `recipe_version` / `ext_version` / `recipe_dir_of` / `sources.sh` | 四份 `${!var}`，且都读 `bundle.conf` |
| 分类显示名 | `PHPRT_EXT_CATEGORIES` + `ext_category_label` | 值域和显示名两张表 |
| 包名迁移提示 | `ext_name_hint` | driver 里硬编码 `sqlite -> sqlite3` |

### 3.3 AI 读契约的痛点

| 问题 | 具体表现 |
| --- | --- |
| 依赖不在头里 | builtin 的依赖藏在 `needs()` 里，要先置 switch=1 执行才知道；`readline -> readline/ncurses` 看头看不出来 |
| 能力限制只能靠试 | `readline` 只能 static，原因在注释里；契约只有 `type=static`，没有“为什么” |
| 文档和校验器两份实现 | `docs/RECIPE.md` 和 `recipe_validate` 各写一遍，加 `category=` 时漏了一处 |
| 枚举值是内部 id | recipe 写 `CacheStorage`，界面显示 `Cache & Storage`，AI 要去 `common.sh` 找映射 |
| 没有机器可读出口 | `ext list --json` 只有 name/kind/version/modes/default/default_set/installed/category |
| 语义坑拦不住 | posix 默认 enabled、ncurses widec、readline `SHLIB_LIBS` 为空，校验器只能查字段 |

### 3.4 今天 8 个 recipe 的踩坑

- `readline` 只能静态：`ext/readline` 用 `PHP_ARG_WITH`，phpize 会强制 libedit。
- `posix` 上游默认 enabled，关闭时必须显式 `--disable-posix`。
- `ncurses` 默认 widec，只产出 `libncursesw.so`；PHP 只认 `libncurses`。
- `ncurses` 的 C++ 绑定在 GCC 15 下编不过，需要 `--without-cxx --without-cxx-binding`。
- `readline` 的 `.so` 默认不链 termcap（`SHLIB_LIBS` 空），需要 `--with-shared-termcap-library`。
- `readline` 依赖 `ncurses`，但看 builtin 头部完全看不出来。

这些都应该变成数据，而不是只留在注释和审计段落里。

## 4. 设计原则

1. **JSON 是对外契约**：字段只增不删；内部重构不能改变 JSON。
2. **schema 是唯一字段表**：字段 × kind 的允许/必填/校验器/默认值只写一次。
3. **accessor 是唯一读取入口**：消费者不直接 `recipe_field "$sh" ini`。
4. **DSL 的 parser 和 validator 是同一个函数**：`opts()`、`wheel=` 各一份。
5. **先警告后报错**：严格校验前先确认现有 77 个 recipe 没有隐藏违规。
6. **不引入重复字段**：`type` 已经表达能力限制，不再加 `shared=no`。
7. **失败要响**：未知字段、重复字段、未定义 note slug 直接报错。

## 5. 目标结构

```text
对外契约（AI/脚本）
  phprt ext list <系列> --json [--name <扩展>]
  └─ 完整、稳定、可 diff 的 JSON

对内契约（driver）
  runner/contract.sh
  ├─ PHPRT_CONTRACT_SPEC        唯一字段表
  ├─ recipe_meta_load           一次解析 + 严格校验
  └─ 字段 accessor

数据
  bundles/<版本>/{builtin,ext,deps,tools}/*.sh
  bundles/<版本>/notes.conf     slug -> 人类可读说明
  bundles/<版本>/aliases.conf   旧名 -> 新名
```

> `runner/contract.sh`、`notes.conf`、`aliases.conf`、`templates/`、`tests/`、
> `docs/RECIPE-HISTORY.md`、`docs/CONTRACT-CHANGELOG.md`、`scripts/doc-check.sh`、
> `scripts/contract-snapshot.sh` 都是计划新增，当前仓库里还没有。

## 6. 对外契约：`ext list --json`

### 6.1 兼容性

- 保持现在是数组，不改成顶层对象。
- 现有字段全部保留：`name/kind/version/modes/default/default_set/installed/category`。
- 新字段只增不删。
- `modes` 继续是字符串（`"static"` / `"static,shared"`），不改成数组。

### 6.2 新增字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `contract_schema` | int | JSON schema 版本，初始 `1` |
| `switch` | string/null | `ENABLE_*`，dep/tool 为 null |
| `modules` | string[] | 产出的 PHP 模块 |
| `ini` | string | `-` 或 `20-<name>.ini` |
| `category_label` | string | 显示名，例如 `Cache & Storage` |
| `menu_order` | int | 分类内排序号 |
| `desc` | string/null | 一行说明，选择器显示 |
| `tools` | string[] | 构建工具 |
| `needs` | object | `declared` / `on` / `off` / `by_opt` |
| `needs_detail` | object | 依赖闭包，含 dep/tool 的版本、依赖、tools、notes |
| `opts` | object[] | `key/state/label/env` |
| `flags_source` | string | `flags()`、`configure_args()` 或 `build()` |
| `flags_on` | string[]/null | switch=1 或默认子项时的 configure 参数 |
| `flags_off` | string[]/null | switch=0 时的 configure 参数 |
| `hooks` | string[] | 实现的函数 |
| `upstream` | string/null | 版本来源 |
| `notes` | object[] | `{slug, text}` |

### 6.3 示例：readline

```json
{
  "contract_schema": 1,
  "name": "readline",
  "kind": "builtin",
  "version": "-",
  "switch": "ENABLE_READLINE",
  "modules": ["readline"],
  "ini": "-",
  "modes": "static",
  "default": "static",
  "category": "Runtime",
  "category_label": "Runtime",
  "menu_order": 1000,
  "desc": null,
  "default_set": false,
  "installed": false,

  "tools": [],
  "needs": {
    "declared": ["readline"],
    "on": ["readline"],
    "off": [],
    "by_opt": {}
  },
  "needs_detail": {
    "readline": {
      "kind": "dep",
      "version": "8.3",
      "needs": ["ncurses"],
      "tools": [],
      "active": true,
      "notes": []
    },
    "ncurses": {
      "kind": "dep",
      "version": "6.5",
      "needs": [],
      "tools": [],
      "active": true,
      "notes": ["ncurses-widec", "ncurses-cxx"]
    }
  },

  "opts": [],
  "flags_source": "flags()",
  "flags_on": ["--with-readline=${DEPS}/readline-8.3"],
  "flags_off": ["--without-readline"],
  "hooks": ["flags", "needs"],
  "upstream": null,
  "notes": [
    {
      "slug": "phpize-libedit",
      "text": "ext/readline 用 PHP_ARG_WITH，phpize 会强制 libedit，只能 static"
    }
  ]
}
```

> 示例是 Phase 4 之后的最终形状。Phase 1 时：
>
> - builtin 还没有头部 `needs=`，`needs.declared` 为 `null`；
> - `note=` 还没启用，`notes` 为空数组；
> - `upstream` 对没有声明的 builtin 为 `null`。

### 6.4 生成规则

- `flags_on/off` 由 driver 调用 `recipe --flags` 得到，不让 AI 猜。
- 实际路径中的 `$DEPS` / `$TREE` 前缀替换成 `${DEPS}` / `${TREE}` 占位符。
- `ext list` 查询前设置合成 `DEPS="$PHPRT_HOME/php-${PHP_VERSION}/deps"` 和
  `BUILD_ROOT="$PHPRT_HOME/build-php-${PHP_VERSION}"`，不要求已安装树。
- 新增 `--name <扩展>` 过滤，避免 AI 一次跑 44 个 recipe。
- 查询前对目标扩展调用 `apply_ext_opts`，按 `opts()` 的默认 `state` 设置
  `ENABLE_<SW>_<KEY>`，保证 `flags/needs` 是确定的默认结果。
- `needs.on` 对 builtin 取 switch=1，`needs.off` 取 switch=0；对 ext 取默认子项，
  `off` 固定为空数组（ext 不选就不构建）。
- `needs.declared` 取头部 `needs=`；builtin 没声明时为 `null`，不是空数组。
- `needs_detail` 递归收集 dep/tool 的头部声明、`needs()` 输出和 `tools`；每项带
  `kind`，`active` 表示当前开关下是否参与。同名 builtin/dep 用 `kind` 区分。
- ext 没有 `flags()` 时，`flags_on/off` 为 `null`，`flags_source` 为 `build()`；
  有 `flags()` 时 `flags_off` 通常是空数组，除非 recipe 显式输出负向参数。
- `opts` 的 `env` 是 `ENABLE_<SW>_<KEY>`，直接给出，AI 不用自己拼。

### 6.5 已知限制

- `flags_on/off` 是查询时刻的快照，依赖 PHP 版本、`bundle.conf` 和子选项。
- `by_opt` 在 Phase 4 才实现；Phase 1 先给空对象。
- ext 的 `flags` 在 Phase 6 才完整；Phase 1 如实返回 `null`。
- `needs_detail` 的 `active` 基于当前开关；要查“所有可能依赖”看 `declared`。

## 7. 内部契约：schema + accessor

### 7.1 唯一字段表

新增 `runner/contract.sh`：

```bash
# key|scope|required|validator|default
PHPRT_CONTRACT_SPEC=(
  "contract|*|*|enum:v2|"
  "kind|*||enum:builtin,ext,dep,tool|path"
  "name|*||name|path"
  "switch|builtin ext|builtin ext|switch|"
  "version|ext dep tool|ext dep tool|version_var|"
  "modules|builtin ext|builtin|module_list|name"
  "type|builtin ext|builtin ext|modes|"
  "default|builtin ext|builtin ext|default_in_type|"
  "ini|builtin ext|builtin ext|ini|"
  "tools|ext dep tool|ext dep tool|resolvable_list|"
  "needs|builtin ext dep|ext dep|resolvable_list|"
  "optional|ext dep|dep|resolvable_list|"
  "wheel|dep||wheel_spec|auto"
  "upstream|*|ext dep tool|upstream|"
  "desc|*||no_space|"
  "category|builtin ext|builtin ext|category|"
  "menu_order|builtin ext||uint|1000"
  "note|*||note_slugs|"
)
```

说明：

- `scope` 不含某个 kind，就是禁止。
- `required` 含某个 kind，就是必填。
- `builtin` 的 `needs` 是新增允许项，语义见 8.1。
- `type` 用 `modes` validator，合法值是 `static`、`shared`、`static,shared`；
  不把逗号写进 enum 字符串，避免和行分隔符混淆。
- `upstream` 的 ext/dep/tool 必填可以用 `versions()` / `latest_version()` 替代；
  validator 负责检查。
- `note` 可选；`note_slugs` validator 在 Phase 4 启用，slug 必须在 `notes.conf`
  中定义。Phase 2 只接受字段，不检查 slug。

### 7.2 规范化加载

```bash
recipe_meta_load <file>
# 结果按文件缓存在 PR_META / PR_LIST 关联数组；accessor 接收文件参数
```

职责：

- 只解析一次头部，不再每个字段起 4 个进程。
- 同一进程内按文件缓存，`recipe_field` shim 不会重复解析。
- 检测缺失头部、多行头部、重复键、未知键。
- 按 SPEC 检查 scope / required / validator。
- `recipe_field <file> <key>` 保留为兼容 shim。
- `recipe_meta`（`--meta`）继续输出排序后的 `key=value`。
- 调用方按文件取用，不依赖“当前 recipe”的全局状态；UI/源码清单循环多个
  recipe 时不会互相覆盖。

### 7.3 accessor

```bash
recipe_switch / recipe_ini / recipe_upstream
recipe_version / recipe_modules
recipe_needs_of / recipe_tools_of
```

目标：`grep -rn recipe_field runner/ scripts/` 只剩 contract 层和 accessor。

### 7.4 错误格式

沿用现有格式：

```text
错误: bundles/php8.5/deps/zlib.sh 不允许字段: category（kind=dep 允许: contract kind name version needs optional tools wheel upstream desc note）
规范: docs/RECIPE.md#3-头部字段
```

## 8. 字段语义决策

### 8.1 builtin 的 `needs=`

**决定**：允许 builtin 在头部写 `needs=`，语义是“可能依赖的超集”。

- `needs()` 是“当前开关/子项下的实际依赖”。
- 校验：`needs()` 的输出必须是头部 `needs=` 的子集。
- `ext list --json` 同时输出 `needs.declared` 和 `needs.on/off`；
  没写 `needs=` 时 `declared` 为 `null`。
- 现有 30 个 builtin 不强制补；新 recipe 模板要求写。
- lint 对“有动态 `needs()` 但没有头部声明”的 builtin 给 warn，推动逐步迁移。

**为什么不是“必须完全一致”**：`gd` 的 jpeg/webp、`curl` 的 sftp 是条件依赖；头部只能写超集，不能表达“当前子项下的实际集合”。

### 8.2 `type` 取代 `shared=no`

**决定**：不新增 `shared=no`。

- `type=static` 已经表达能力限制。
- “为什么只能 static”用 `note=phpize-libedit` 表达。
- 再加 `shared=no` 会形成两个字段表达同一件事，增加 schema 面积和漂移点。

如果将来 UI 需要机器可读的原因分类，再加 `mode_reason=`；不在本轮。

### 8.3 `note=` 与 `notes.conf`

- recipe 头写 `note=slug1+slug2`，不能带空格。
- `bundles/<版本>/notes.conf` 存 `slug=人类可读说明`。
- validator 检查 slug 格式和是否定义。
- `ext list --json` 输出 `[{slug, text}]`。

这样既有机器可读的短标识，也不会让 AI 只看到 `phpize-libedit` 这种缩写。

### 8.4 `optional` 的处理

**现状**：`optional` 只参与校验，不参与构建；条件依赖实际写在 `needs()` 里。

**决定**：本轮不改行为，只把文档写实：

- 明确“`optional` 是声明性元数据，不参与 resolver”。
- 条件依赖必须写在 `needs()`。
- `contract=v3` 时再决定删除或赋予语义。

### 8.5 版本来源

**现状**：driver 在 `bundle.conf` 里按变量名取值，不是文档说的 `recipe_main --version`。

**决定**：

- `recipe_version` 统一：先读 `bundle.conf`，取不到再跑 `bash recipe --version`，进程内缓存。
- `recipe_source_meta` 统一输出 `name<TAB>version<TAB>url<TAB>sha`。
- 实现文档里写了但代码没有的 `download_url` / `checksum` 钩子，并在
  `recipe_main` 增加对应动词。
- `sources.sh` 不再自己读 `${!var}`。

### 8.6 ext 的 `flags()` / `configure_args()`

**决定**：契约允许 ext 实现可选的 `flags()` 或 `configure_args()`，输出
`ext_pecl_build` 的 configure 参数。

- 新 recipe 模板要求写；现有 13 个 ext 已实现 `configure_args()`，其余按需迁移。
- JSON 的 `flags_source` 按实际钩子输出 `flags()` / `configure_args()` / `build()`。
- 有钩子时，`flags_on` 是构建时的 configure 参数；`flags_off` 通常是空数组，
  除非 recipe 显式输出负向参数（ext 不选就不构建，负向参数一般不适用）。
- AI 不用读 `build()` 就能拿到 configure 参数。

### 8.7 `switch` 使用规则按 kind 区分

**决定**：

- builtin：`flags()` / `needs()` 必须引用 `switch`。
- ext：允许不引用；`switch` 由 driver 设置环境变量，build 是否读由 recipe 决定。

原因：基线有 14 个 ext 在体内不引用 switch；现在的检查因为命中头部才恒真。

## 9. 分阶段计划

### Phase 0：止血 + 事实修正（0.5 天）

改动：

- 修 `scripts/recipe-check.sh` 的 `osw ... obad=0`，补 `local`。
- `menu_order` 加校验：只允许 builtin/ext、`^[0-9]+$`、缺省 1000。
- 给 `category` 加 kind 限制：只允许 builtin/ext；显示名合并成
  `PHPRT_CATEGORY_LABELS`。
- `ext_name_hint` 移到 `bundles/php8.5/aliases.conf`，一行 `旧名=新名`
  （例如 `sqlite=sqlite3`），提示文案由 driver 统一生成。
- 文档修正：L1 规则补 category/menu_order；把“110 个 legacy”段落移到
  `docs/RECIPE-HISTORY.md` 或删除；`version` 实际读 `bundle.conf`；
  `upstream` 对 ext 实际也是必填（当前代码如此）。

验收：

- `scripts/recipe-check.sh` 通过且 stderr 为空。
- 非法 `menu_order`、dep 上的 `category` 都会失败。
- `--ext sqlite` 仍有迁移提示。

### Phase 1：`ext list --json` 完整契约（1.5 天）

改动：

- 扩展 `cmd_ext_list` 的 `--json` 输出。
- 新增 `recipe_query_with_switch <sh> <0|1> <verb>`。
- 新增 `recipe_needs_closure <name>`。
- 新增 `recipe_flags_json <sh>`，做路径占位符替换。
- 新增 `--name <扩展>` 过滤。
- `hooks` 用 `recipe_has_func` 枚举。
- 写 JSON fixture 和 jq 校验。

验收：

- `phprt ext list 8.5 --json --name readline` 能看到 `needs_detail.readline.needs=["ncurses"]`。
- JSON 对未安装树也能生成。
- 现有 `--json` 旧字段一个不少。
- JSON fixture 通过。

### Phase 2：schema + 规范化加载（1.5 天）

改动：

- 新增 `runner/contract.sh`。
- `recipe_validate` 改成遍历 `PHPRT_CONTRACT_SPEC`。
- 未知键、重复键、禁止项、必填项统一处理。
- `recipe_field` 保留兼容 shim。
- 扩展 `--meta` 的 golden 测试。

验收：

- 77 个 recipe 全过。
- 缺字段、禁止字段、未知字段、重复字段、坏 `menu_order`、坏 `category` 全部按统一格式失败。
- `--meta` 输出与改造前逐字节一致。

### Phase 3：accessor 收口（1 天）

改动：

- 新增 `recipe_switch` / `recipe_ini` / `recipe_upstream` / `recipe_version` / `recipe_modules` / `recipe_needs_of` / `recipe_tools_of`。
- 替换消费者侧的 14 处字面 `recipe_field` 读取（build 2、commands 6、sources 5、upstream 1）。
- `scripts/recipe-check.sh` 的 4 处也改走 accessor。

验收：

- `grep -rn recipe_field runner/ scripts/` 只剩 contract 层和 accessor。
- 所有 recipe 的 `--meta`、`--needs`、`--version` 快照不变。
- Phase 1 的 JSON 输出不变。

### Phase 4：依赖语义 + 坑数据（1 天）

改动：

- 允许 builtin 写 `needs=`，实现子集校验。
- 新增 `bundles/<版本>/notes.conf`。
- 允许 `note=`，实现 slug 校验和 JSON 输出。
- 给 readline / ncurses / posix 等 recipe 补 `note=`。

验收：

- readline 的 JSON 能看到“只能 static”的原因。
- posix 的 JSON 能看到“默认启用，关闭要显式 disable”。
- 未定义 slug 直接失败。
- `needs()` 输出超出头部声明时失败。

### Phase 5：lint + 文档 + 模板 + AGENTS（1.5 天）

改动：

- `recipe-check.sh` 增加 flags 成对检查：`--enable/--disable`、`--with/--without`；
  规范化时去掉 `=...` 值。静态 warn；`--deep` 只在 PHP 源码已解压时用
  `configure --help` 确认参数存在后再 error，没有源码则 warn 跳过。
- `category` 报错带可选值和示例，例如：
  `category 不合法: Cache (可选: Database CacheStorage ...；示例: category=CacheStorage)`。
- `phprt verify` 增加 `modules` 与 `php -m` 对比，只检查 `tree_exts` 返回的已启用扩展。
- 文档数字用 marker + `scripts/doc-check.sh`，覆盖 `README.md`、
  `docs/EXTENSIONS.md`、`docs/TEST-V2.md`，例如：
  `<!-- phprt:count builtin -->30<!-- /phprt:count -->`。
- `docs/RECIPE.md` 拆成 normative + `docs/RECIPE-HISTORY.md`，并修正内部链接。
- 字段矩阵由 `PHPRT_CONTRACT_SPEC` 生成，插在 marker 里。
- 新增 `docs/CONTRACT-CHANGELOG.md`，记录 `category` / `menu_order` / `note` 是哪一版加入。
- 新增 `templates/` 5 个模板：
  `builtin-no-deps.sh`、`builtin-with-dep.sh`、`builtin-with-opts.sh`、
  `dep-autotools.sh`、`pecl-ext.sh`，以及 `tests/templates.sh`。
- `AGENTS.md` 增加“契约读取顺序”，内容见 9.8。

验收：

- recipe-check 无噪声。
- doc-check 通过。
- 模板测试通过。
- AI 只读 JSON 能回答 readline 的依赖、模式限制和坑。

### Phase 6：版本/源码 + opts/wheel + ext flags（1.5 天）

改动：

- `recipe_version` 统一，支持 recipe 内版本。
- `recipe_source_meta` 统一 URL/SHA。
- 实现 `download_url` / `checksum` 钩子。
- `recipe_opts_meta` 唯一 validator；recipe-check 复用；新增 `recipe_opts_usage_check`。
- `wheel_parse` 统一 parser。
- ext 可选 `flags()` / `configure_args()`；已有 13 个 `configure_args()` 已进入
  JSON，swoole/xdebug 等按需补。

验收：

- 同一个坏 `opts()` 在运行时和 recipe-check 都报错。
- 同一个坏 `wheel=` 在 L1 和 wheelize 都报错。
- 临时把某个版本变量移进 recipe，`dep_dir_of`、签名、下载都能取到。
- ext 的 `flags` 在 JSON 里可见。

### Phase 7：契约版本化 + fixtures（1 天）

改动：

- `recipe_meta_load` 按 `contract=v2|v3` 选 decoder；先只实现 v2，留 v3 接缝。
- `tests/contract/` fixtures：合法/缺字段/禁止字段/未知字段/重复字段/坏 category/坏 menu_order/坏 note/坏 opts/坏 wheel/末行/顶层代码/v3。
- `scripts/contract-snapshot.sh` 生成快照。

验收：

- fixtures 全过。
- 快照 diff 为空。
- 未知 contract 按统一错误失败。

### 9.8 给 AI 的读取顺序（写入 `AGENTS.md`）

1. `phprt ext list <系列> --json --name <扩展>` 取完整契约；不要先读
   `common.sh` 或逐个 `bash recipe.sh --flags`。
2. 读 JSON 里的 `hooks`、`needs_detail`、`opts`、`flags_on/off`、`notes`；
   需要背景时再读目标 recipe 的头部。
3. 实跑四件事：`--check`；`--flags`（builtin 的 switch=0 和 1 各一次）；
   `--needs`；`--opts`。
4. 只把 `docs/RECIPE.md` 的 normative 段当契约；`docs/RECIPE-HISTORY.md`
   和 `docs/EXTENSIONS.md` 的台账只当背景。
5. 改完跑 `scripts/recipe-check.sh`；构建后跑 `phprt verify`；数字类文档由
   `scripts/doc-check.sh` 兜底。

## 10. 测试与门禁

### 10.1 现有门禁

```bash
scripts/recipe-check.sh
scripts/recipe-check.sh --deep
bash -n runner/*.sh scripts/*.sh
phprt verify
```

### 10.2 新增测试

```bash
tests/contract/run.sh          # schema 正反 fixtures
tests/templates.sh             # 5 个模板替换后 --check
scripts/doc-check.sh           # 文档数字与 JSON/文件数一致
scripts/contract-snapshot.sh   # --meta/--version/--needs 快照
```

测试使用临时 `PHPRT_HOME` / `BUNDLE`，不读写真实项目 `.phprt`；fixtures 不依赖
网络和已安装树。

### 10.3 验收矩阵

| 命令/检查 | 期望 |
| --- | --- |
| `scripts/recipe-check.sh` | 77 个全过，stderr 为空 |
| `phprt ext list 8.5 --json --name readline` | 含 `needs_detail`、`flags_on/off`、`hooks`、`notes` |
| JSON fixture | 通过 |
| `tests/contract/run.sh` | 正反 fixtures 符合预期 |
| `scripts/contract-snapshot.sh` diff | 空 |
| `scripts/doc-check.sh` | 文档数字一致 |
| `tests/templates.sh` | 模板可用 |
| `phprt verify` | 现有树全绿，modules 对比通过 |
| `grep recipe_field runner scripts` | 只剩 contract 层/accessor |
| `grep -rn 'ext_name_hint' runner scripts` | 无命中；包名提示已走 `aliases.conf` |
| 人工检查 `libssl.so.3` / `sqlite3` 判断 | 只剩注释示例，没有条件分支 |

## 11. 风险与回滚

| 风险 | 缓解 |
| --- | --- |
| 严格校验误伤现有 recipe | 先 `PHPRT_CONTRACT_WARN=1` 只报告，再切默认失败 |
| JSON 生成慢 | `--name` 过滤；只对目标 recipe 跑 flags/needs |
| flags 路径依赖项目 | 输出 `${DEPS}` / `${TREE}` 占位符 |
| ext flags 迁移工作量大 | 先允许 `null`，新 recipe 必写，旧 recipe 逐步迁移 |
| 文档和 schema 再次漂移 | 字段矩阵由 schema 生成，doc-check 对比 |
| recipe 内版本下沉破坏签名 | `recipe_version` 统一入口，先在测试 recipe 验证 |
| `note=` 变成第二个文档 | slug 必须进 `notes.conf`，未定义直接失败 |

回滚策略：

- 每个 Phase 独立提交，可单独 revert。
- `recipe_field` shim 保留到 Phase 3 结束，回滚不影响旧调用。
- JSON 只增字段，回滚不会破坏消费者。

## 12. 待拍板

1. **`needs=` 的语义**：是否接受“头部是声明超集，`needs()` 是实际依赖，校验子集”？
   如果坚持“必须完全一致”，gd 的 jpeg/webp、curl 的 sftp 这类条件依赖无法用头部表达。

2. **ext 的 `flags()`**：是否把“ext 必须能吐出 configure 参数”写进契约？
   不写，JSON 对 ext 永远是 `flags:null`；写，需要迁移 14 个 ext，但 AI 才能真正不读 `build()`。

## 13. 附录：字段矩阵

| 字段 | builtin | ext | dep | tool | 说明 |
| --- | --- | --- | --- | --- | --- |
| `contract` | 必填 | 必填 | 必填 | 必填 | `v2`（未来 `v3`） |
| `kind` | 可选 | 可选 | 可选 | 可选 | 路径推断，写则必须一致 |
| `name` | 可选 | 可选 | 可选 | 可选 | 路径推断，写则必须一致 |
| `switch` | 必填 | 必填 | 禁止 | 禁止 | `^ENABLE_[A-Z0-9_]+$`；builtin 体内必须引用，ext 可只由 driver 使用 |
| `version` | 禁止 | 必填 | 必填 | 必填 | 变量名，值在 `bundle.conf` 或 recipe |
| `modules` | 必填 | 可选 | 禁止 | 禁止 | 逗号分隔；ext 默认 name |
| `type` | 必填 | 必填 | 禁止 | 禁止 | `static` / `shared` / `static,shared` |
| `default` | 必填 | 必填 | 禁止 | 禁止 | 必须是 `type` 子集 |
| `ini` | 必填 | 必填 | 禁止 | 禁止 | `-` 或 `NN-<name>.ini` |
| `tools` | 禁止 | 必填 | 必填 | 必填 | 可空 |
| `needs` | 可选 | 必填 | 必填 | 禁止 | builtin 是声明超集；ext/dep 是硬依赖 |
| `optional` | 禁止 | 可选 | 必填 | 禁止 | 声明性元数据，不参与 resolver |
| `wheel` | 禁止 | 禁止 | 可选 | 禁止 | `auto` / `none` / 指令 |
| `upstream` | 可选 | 必填 | 必填 | 必填 | 可为 `none`，或用 `versions()` / `latest_version()` |
| `desc` | 可选 | 可选 | 可选 | 可选 | 不能带空格 |
| `category` | 必填 | 必填 | 禁止 | 禁止 | 值域见 `PHPRT_CATEGORY_LABELS` |
| `menu_order` | 可选 | 可选 | 禁止 | 禁止 | 非负整数，缺省 1000 |
| `note` | 可选 | 可选 | 可选 | 可选 | `slug1+slug2`，slug 必须在 `notes.conf` |

## 14. 评审记录

评审日期：2026-11-10。以下问题已在本文修正：

1. **调用数写错**：原文写“24 处”，实际 `grep recipe_field` 是 50 处调用，消费者侧
   14 处；已改为准确口径。
2. **`type` 的 enum 表示有歧义**：`static,shared` 本身含逗号，不能放进逗号分隔的
   enum。已改为 `modes` validator，并单独说明合法值。
3. **`upstream` 必填范围和代码不一致**：代码要求 ext 也有 upstream，原文写 ext
   可选。已把矩阵和 schema 改成 ext/dep/tool 必填，并在 Phase 0 同步文档。
4. **Phase 0 验收和改动不一致**：原文说 dep 上的 `category` 会失败，但 Phase 0
   只加 `menu_order` 校验。已把 category kind 限制并入 Phase 0。
5. **ext 的 `flags_off` / `needs.off` 语义不清**：已明确 ext 不选就不构建，
   `flags_off` 通常为空数组，`needs.off` 固定为空。
6. **builtin 的 `needs.declared` 没声明时行为不清**：已明确为 `null`，不是空数组。
7. **示例和阶段状态混在一起**：JSON 示例看起来像 Phase 1 就能产出，实际包含
   Phase 4 的 `needs.declared` 和 `notes`。已加说明。
8. **`note_slugs` 在 Phase 2 没有 `notes.conf`**：已明确 Phase 2 只接受字段，
   Phase 4 才启用 slug 校验。
9. **缺少 AI 读取顺序**：已补 9.8，并写明写入 `AGENTS.md`。
10. **缺少模板清单、changelog、doc marker 示例**：已补到 Phase 5。

仍然待拍板：

- `needs=` 用“声明超集 + 子集校验”，还是要求与 `needs()` 完全一致。
- ext 是否必须实现 `flags()`；不实现时 JSON 的 ext flags 永远为 `null`。

