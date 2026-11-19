# 锁定与旧补丁

状态：已实现。命令和错误文案见 [CLI.md](CLI.md)，state 字段见 [STATE.md](STATE.md)，
子命令流程见 [FLOW.md](FLOW.md)，标记字段见 [BUILD.md](BUILD.md)。

本文定义 `--lock`、`scheme --lock`、默认版本复用、`update` 语义和 catalog 旧补丁保留规则。
state 是锁的唯一事实来源，没有独立 lock 文件。

## 1. 问题

`state.json` 记录了每个组件和依赖实际使用的 `line` / `version`，但早期解析时不使用：

```text
catalog 里 openssl 3.5 线新增 3.5.10
  → bind::resolve 取该线最大补丁 3.5.10
  → dep 构建签名变化
  → openssl 重编
  → 产品构建签名变化
  → PHP 跟着重编
  → state.deps.version 被新版本覆盖
```

`add` / `remove` 还会重新解析产品补丁：目标写 `php@8.5` 时总是取该线的最大补丁，
检查安装树却用 `installed_version`。catalog 升到 8.5.13 后，`add` 会装出一棵新的
8.5.13 树，旧树保留，`installed_version` 被改掉。

其他相关问题：

- `pins` 只来自 `bindings + CLI`，bundle 默认线每次重算；bundle 改默认线后，已有项目
  的 `update` 也会换线。
- `remove` 不重编（shared / dynamic）时，也会用当前 catalog 的结果重写 `state.deps`，
  记下并没有真正编译过的版本。
- 完成标记里的 scheme 指纹不含版本，同一方案、不同补丁会被判成“配置一致”。
- `scheme` 只导出用户显式钉的版本线，产品补丁、实际解析出的线、依赖补丁都不导出。

## 2. 目标

| 模式 | 锁定内容 | 补丁 |
| --- | --- | --- |
| 默认（不带 `--lock`） | 当前方案中已有节点的 line；产品补丁用 `installed_version` | 仍取该线最大补丁 |
| `--lock` | 在默认基础上，把当前方案中已有节点锁到 `state.*.version` | 不跟随 catalog 升级；缺失就报错 |
| `update` | 保持 Scheme、保持 version line | 为当前闭包重新取 catalog 最新补丁 |

另外：

- 不新增独立 lock 文件，state 仍是唯一来源；
- `plan` 能直接验证默认解析、`--lock` 解析和 `update` 前后的版本差异；
- catalog 每条线保留全部旧版本，作为 `--lock` 和显式旧版本安装的源码来源。

不在本次范围：

- 独立 `phprt.lock` 文件；
- recipe hash、编译器 hash、环境快照；
- 完整 reproducible build、gcc/tool 锁定；
- 把 `url` / `sha256` 复制进 state。

## 3. 解析顺序

### 3.1 Effective Pins 只覆盖当前闭包

正确顺序：

```text
CLI / current scheme
        ↓
确定 desired components
        ↓
计算当前 desired dependency closure
        ↓
与已有 state 做 intersection
        ↓
生成 Effective Pins
        ↓
resolver
        ↓
plan
```

定义：

> `--lock` 只锁定“当前待解析方案中，同时已经存在于 state 的节点”。

不能先按整份 `state.components` / `state.deps` 生成 pins 再套当前方案：那样会把已经
remove 的组件和它们的历史依赖带回解析。

当前方案（desired components）由命令决定：

| 命令 | desired components |
| --- | --- |
| `plan` / `install` / `update` | `state.components` ∪ CLI 新增，减去 `--disable-default-component` |
| `add` | 同上，加上待加入的组件 |
| `remove` | 先从 `state.components` 删掉待移除组件，再算 |
| `scheme --lock` | `state.components` |

依赖闭包由 `plan::resolve` 对当前选择和 feature 求解得到。只有闭包里的名字才可能
进入 pins：

- 外部组件：`state.components[name].line` / `.version`；
- 依赖库：`state.deps[name].line` / `.version`；
- `state.bindings` 里的显式线，且名字必须在闭包内；
- bundled 组件不参与 pin：`state.components[name].version` 是产品补丁，源码在产品
  tarball 里；同名依赖库（如 dep openssl）仍按 `state.deps` 参与。

已删除组件的残留不会进入 pins：

```text
state（旧命令或手改留下的）:
  components: php, swoole, openssl
  deps:       openssl 3.5 / 3.5.9
              libxml2 2.15 / 2.15.0      ← 已删除组件以前用的依赖

current scheme:
  php, swoole, openssl   （闭包里没有 libxml2）

phprt plan php@8.5 --lock
  → pins: swoole、openssl；libxml2 不参与
  → catalog 里没有 libxml2 2.15.0 也不会报错
```

如果 libxml2 真的回到当前闭包（有组件重新需要它），`--lock` 才会要求 catalog 里有
2.15.0；没有就报错。

### 3.2 两个 pass

闭包要用“选好的版本线”才能算，pin 又要用闭包才能生成，所以解析分两遍：

1. 第一遍按 CLI 显式线、`state.bindings`、bundle 默认线等确定 line，补丁仍取 catalog
   当前，加载 recipe，算出当前 `Selection`、feature 和构建顺序 `order`；`order` 里的
   名字就是 desired 闭包。catalog 里已经找不到的 line 在这一遍不报错，只是不参与取线。
2. 用闭包和 state 求交生成 `Effective Pins`。这一遍会把 `state.components[].line` /
   `state.deps[].line` 也纳入 pins，所以**第二遍本身可能发生 line 变化**（例如 bundle
   默认线升级过，或依赖线当初是自动选出来的）。pin 和第一遍结果有任何差异时，重新
   `bind::resolve` + 重新加载 targets，再重算 `Selection` / `preset` / 产品目标。
3. 闭包校验（`main.rs::check_closure_change`）：第二遍重载后用同一份 `Selection` /
   feature 再跑一次 `plan::resolve`，只用于检查——不重新 bind、不重新生成 pins，也
   不做第三遍解析。比较的是节点集合和每个节点的 `children()` 输入快照
   （`plan::inputs`），不比较 order 的 Vec（拓扑序本来就可能变）。节点集合不同：
   `--lock` 直接报错（锁住的不是最终闭包），普通命令打“注意”后按第二遍结果继续；
   集合相同但某节点的 plan 输入变了：只打“注意”，因为 pins 本身仍然精确。

两遍之后不再迭代。当前两遍足够的关键不变量是：

```text
version 不参与 dependency closure
```

`pins.versions` 只影响最终选中的 artifact（version / url / sha256）和构建元数据，
不改变 `needs` / `features` / `tools`，因此不会改变闭包。闭包只由产品、系列、方案
组件集、mode、feature / deps 和 line 决定。

Recipe 契约：`target(ctx)` **不能根据 `self["version"]` /
`ctx["deps"][*]["version"]` 改变 `needs` / `features` / `tools`**，也不能通过其他字段
间接改变闭包。当前 recipe 里 version 只用于选择 artifact 和写构建元数据。违反这条
契约时，第二遍重载后的闭包可能和生成 pins 时用的闭包不同；`--lock` 路径现在会被第
3 步拦住（报错），普通路径只警告，代码里没有任何地方会自动再迭代一轮。

## 4. 默认行为

目标写 `<产品>@<系列>`（不是显式补丁），且 state 有 `installed_version` 时：

1. 产品补丁 = `state.installed_version`；`update` 除外。
2. 已有外部组件的 line = `state.components[].line`。
3. 已有依赖的 line = `state.deps[].line`。
4. bundled 组件的 line 不参与锁定；它的源码在产品 tarball 里，版本跟产品补丁。
5. 没有 state 记录的新组件 / 新依赖，按 bundle 默认线或唯一可用线解析。
6. 补丁仍按 catalog 当前值取。

优先级：

```text
CLI 显式产品补丁 / --dep / version:
  > state 记录的 line
  > bundle 默认线
  > 只有一条线时自动使用
```

示例：

```bash
# catalog 把 php 8.5 升到 8.5.13，state 里装的是 8.5.12
phprt plan php@8.5
# product:php@8.5.12；已有 openssl 仍走 state 的 3.5 线，补丁取 catalog 当前 3.5.x
```

`install php@8.5` 因此不会再提示“已存在旧版本”，也不会自动升级；升级统一走
`phprt update php@8.5`。

显式写产品补丁仍然是覆盖：

```bash
phprt install php@8.5.13        # 明确要 8.5.13
phprt update php@8.5            # 跟随 catalog 升级到 8.5 线最新补丁
```

### 4.1 记录缺失

默认模式下，state 记录的产品补丁在 catalog 里不存在时直接报错，不回落最新版本：

```text
state 里的 php 8.5.12 不在 catalog（8.5 线可用：8.5.13, 8.5.12）；
用 phprt update php@8.5 升级，或在 catalog 里保留旧版本
```

### 4.2 完整方案列表

`install <产品>@<系列> '组件列表'` 会替换当前组件方案：

- 不再使用旧 `state.components` / `state.deps` 的 line 作为 pin；
- 列表里的 `version:` / `--dep` 和 bundle 默认线决定新方案；
- 产品补丁仍按第 4 节处理（已有 `installed_version` 时保持）。

## 5. `--lock`

`--lock` 在默认行为上再锁补丁。

### 5.1 支持的命名

```bash
phprt plan    php@8.5 --lock [--json]
phprt install php@8.5 --lock
phprt add     php@8.5 swoole --lock
phprt remove  php@8.5 swoole --lock
phprt scheme  php@8.5 --lock
```

`update` 不接受 `--lock`；它就是升级入口。

### 5.2 规则

1. 必须有 `installed_version`；没有安装完成的产品时报错：

   ```text
   当前没有安装完成的产品，--lock 没有可锁定的版本；
   先 phprt install php@8.5，或用 phprt scheme php@8.5 --lock 导出源项目的精确方案
   ```

2. 只锁第 3 节定义的闭包内、且 state 里有 `version` 记录的节点。
3. 新加入、state 里没有记录的节点仍取该线最大补丁；`--lock` 只锁已有记录。
   `scheme --lock` 例外：它要导出可复现命令，闭包内没有 state 记录的节点会用本次
   解析实际选中的 artifact version 补齐（见第 7 节）。
4. 该线的补丁数组里找不到锁定版本时报错，不回落：

   ```text
   lock 里的 zlib 1.3.2 不在 catalog（线 1.3 可用：1.3.3）；
   用 phprt update php@8.5 升级，或在 catalog 里保留旧版本
   ```

   这条不能退回最大补丁：`--lock` 的意义就是精确复用已有源码。回归测试见
   第 11.1 节第 3 条。

5. CLI 显式选择整体覆盖 binding（线 + 补丁一起换），不是只改补丁字段：

   ```text
   state:     openssl line=3.5 version=3.5.9
   CLI:       --dep openssl=4.0
   resolver:  openssl line=4.0 version=<4.0 线最大补丁>
   ```

   ```text
   state:     openssl line=3.5 version=3.5.9
   CLI:       --dep openssl=3.5.9
   resolver:  openssl line=3.5 version=3.5.9（反查出所属线）
   ```

   不允许出现 `line=3.5` 加 `version=4.0.x` 这种跨线组合：换线时必须丢掉同名的旧
   补丁 pin，换补丁时必须同时确定它所属的线。组件用 `version:` 走同一套规则。

   CLI 值的解析顺序：先按 catalog 的 line 精确匹配；不匹配时在各线的补丁数组里
   按版本号反查所属线；都找不到就报错并列出可用版本。

6. 显式产品补丁：产品按显式补丁，其他已有节点仍按 lock。
7. `--lock` 不改变构建必要性。static / fold 组件改 PHP configure 时，PHP 仍会重编；
   `--lock` 只避免已有依赖被升级而多出的重编。
8. 构建工具也按 `state.tools` 锁定：`provider = "store"` 的 built 版本优先于宿主，
   保证“上次真的编过的 autoconf 版本”能被复现。同一名字有多个锁定版本时，取满足
   当前区间的最高的那个；锁定版本不满足当前要求、或不在 catalog 里时直接报错，
   不静默回退。宿主工具不锁版本，换机器重新探测。

### 5.3 和完整方案列表冲突

`--lock` 和完整方案列表不能一起用：

```bash
phprt install php@8.5 --lock 'curl=mode:static,...'   # 报错
```

方案列表已经可以带精确版本；两个来源同时决定版本会互相覆盖。换机器复现用
`scheme --lock` 生成的命令。

## 6. `update`

语义固定为：

```text
update =
  保持当前 Scheme
  + 保持当前 version line
  + 重新为当前 dependency closure 选择 catalog 中最新 patch
```

例如：

```text
before:
php       8.5.12
swoole    6.2.5
openssl   3.5.9
zlib      1.3.2

catalog:
php       8.5.13
swoole    6.2.6
openssl   3.5.10
zlib      1.3.3

phprt update php@8.5
→ php       8.5.13
  swoole    6.2.6
  openssl   3.5.10
  zlib      1.3.3
```

- line 不变：`swoole` 仍在 6.2，`openssl` 仍在 3.5，`zlib` 仍在 1.3；
- 产品补丁取 catalog 当前（8.5 线 8.5.13）；
- 成功后新版本写进 state，成为下一次 `--lock` 的精确基准：

  ```text
  update        → state: openssl 3.5 / 3.5.10
  add --lock    → resolver 用 3.5.10（不是 3.5.9）
  ```

- catalog 删掉了 state 记录的线时直接报错，不静默换线；
- `update` 不受旧 `installed_version` 约束，否则它就升不动了。

## 7. `scheme --lock`

普通 `scheme` 只导出方案；`scheme --lock` 导出当时的解析结果。它会真正走第 3.2 节
的两遍解析（`uses_resolver` 对 `scheme --lock` 生效），所以 state 里的线不在 catalog
时，报的是 `bind::resolve` 的错；`bindings` 用的是第二遍结束后的最终值。

```bash
phprt install php@8.5.12 \
  'openssl=mode:static,feature:[argon2],curl=mode:static,dep:libcurl[ssl],swoole=mode:shared,version:6.2.5,zlib=mode:static' \
  --dep libcurl=8.23.0 --dep openssl=3.5.9 --dep zlib=1.3.2
```

| 对象 | 导出方式 |
| --- | --- |
| 产品 | 目标写成 `产品@补丁` |
| 有自己版本线的外部组件 | `version:<补丁>`（catalog 里有旧补丁时写补丁） |
| bundled 组件 | 不写 `version:`，源码在产品 tarball 里 |
| 依赖库 | `--dep 名字=<补丁>` |
| mode / feature / dep 转授 | 和普通 `scheme` 一样 |
| 构建目标 | `--targets '...'`，来自 state.targets 或 bundle 默认值 |

- 只导出当前闭包里的节点：已删除组件留在 `state.deps` / `state.bindings` 的历史记录
  不写进命令；`bindings` 在渲染前按闭包过滤；
- 要求 state 有 `installed_version`，否则报错；
- 每个旧补丁都必须能在 catalog 里找到；找不到时直接报错，不生成一条执行会失败的假 lock；
- 版本来源是 state 记录优先；闭包里 state 没有 `version` 记录的节点，用本次解析
  实际选中的 artifact version 补齐，不允许整条 pin 漏掉（比如 `state.deps` 缺一条
  记录时，闭包里的依赖仍然会被 `--dep 名字=<补丁>` 导出）；
- bundled 组件的 `version` 是产品补丁，永远不会被当成它自己的源码版本；同名依赖库
  （如 dep openssl / dep zlib）仍按依赖导出。

## 8. catalog 的补丁数组

适用于产品、组件和依赖库的所有版本线。

每条线是一个数组，保存这条线发布过的所有补丁；默认选择取数组中版本最大的补丁，
和数组顺序无关：

```toml
[zlib]
"1.3" = [
    { version = "1.3.3", url = "https://zlib.net/fossils/zlib-1.3.3.tar.gz", sha256 = "..." },
    { version = "1.3.2", url = "https://zlib.net/fossils/zlib-1.3.2.tar.gz", sha256 = "..." },
]
```

规则：

- 新版本直接加进数组，旧版本保留不删除；
- 每个元素的 `url` / `sha256` 必须是该版本的真实值，不能只写版本号；
- 默认取最大版本；显式安装旧版本、`--lock`、`scheme --lock` 按版本精确查找；
- state 只记版本，不复制 `url` / `sha256`；catalog 是唯一源码来源；
- 旧版本缺失时命令直接报错，不自动改用最大版本。

## 9. resolution digest

完成标记里的 `resolution=` 用来判断“方案相同，解析出的补丁是否相同”。它必须与节点
收集顺序无关，因此先规范化再哈希：

1. 节点按稳定 canonical key 排序：`Target::id()`，即 `product:<名字>@<补丁>`、
   `component:<名字>`、`dep:<名字>`；
2. 每个节点字段顺序固定：`kind`、`name`、`line`、`version`、`sha256`；
3. 序列化格式固定：`product=<产品补丁>` 一行，然后每个节点
   `node=<id>` + 四行 `key=value`；
4. 对这段文本算 SHA-256。

```text
php / openssl / zlib  与  zlib / openssl / php
→ 语义相同 → resolution= 完全相同
```

实现见 `marker::resolution_digest`，单元测试见第 11.2 节。

边界：`resolution` 只用于比较，不作为解析来源。marker 不存 line、不存依赖版本；
版本来源仍然只有 `state`，否则一个系列有多棵树时无法确定该听哪棵。

## 10. 实现落点

### 10.1 模块

| 文件 | 职责 |
| --- | --- |
| `src/lock.rs` | `EffectivePins`、`CliPin`、`cli_pin`、`effective_pins`、`exact_versions` |
| `src/bind.rs` | `resolve(..., pins: &EffectivePins)`；精确补丁从 `catalog.artifact_for` 取，缺失即报错 |
| `src/main.rs` | `pins_from_cli`、`pin_lines`、`lock_requested`、`uses_resolver`、`select_product_patch`；两遍解析 |
| `src/state.rs` | `prune_deps`：不重编的 remove 保留版本、只收窄闭包；`prune_bindings`：remove 后按最终闭包收窄显式线 |
| `src/marker.rs` | `resolution` 字段、`resolution_digest`、`version=` 校验、统一 `render_scheme` |
| `src/build.rs` | 写标记时计算 `resolution` 并显式传产品补丁 |

`EffectivePins` 是两个 `BTreeMap<String, String>`：

```text
lines:    名字 -> 版本线      （默认和 --lock 都生效）
versions: 名字 -> 精确补丁    （只在 --lock 时填）
```

产品补丁不放进 pins，单独由 `select_product_patch` 决定：

```text
显式 target 补丁 > update 取 catalog 当前 > 默认取 installed_version
```

### 10.2 各命令的 state → resolver → state

所有会解析的命令（`graph` / `plan` / `install` / `update` / `add` / `remove`，以及
`scheme --lock`）共用第 3.2 节的两遍流程；区别只在写不写 state。普通 `scheme` 不解析。

`install`：

```text
读 state → 产品补丁（显式 / installed_version）
  → 方案 = state.components ∪ CLI
  → 两遍解析（pins → bindings → recipe → plan）
  → 现场判定（重复安装 / 中断 / --force）
  → 下载、解压、编译
  → 写标记（version + scheme + resolution）
  → 写 state（components line/version、deps、bindings、installed_version）
```

`add`：

```text
读 state → 目标组件加进方案
  → 两遍解析（新组件不在 state，不产生 pin；已有节点按默认或 --lock 复用）
  → 检查 ini / 产物 / mode
  → 只在方案里、没装过：写 state，提示先 install
  → 否则编译（shared 只编组件，static 重编产品）
  → 写标记 → 写 state
```

`remove`：

```text
读 state → 组件先从方案删掉，再解析（它和它的独占依赖不在 desired 里，不产生 pin）
  → 检查 ini / 产物
  → 两种路径都按最终闭包收窄 state.bindings（prune_bindings）；
    生成 scheme 指纹 / 命令时用的就是收窄后的 bindings
  → 不重编：删 ini / .so，state 保留 deps 的 line/version，
    只收窄 feature、删闭包外的 dep（prune_deps）
  → 要重编：走完整流水线，按本次解析结果覆盖 deps
  → 不重编时用 installed_version 重写标记的组件列表和 scheme
```

`update`：

```text
读 state → 产品补丁取 catalog 当前
  → 方案 = state.components，line 全部按 state
  → 两遍解析（没有版本 pin，补丁取 catalog 当前）
  → 编译 → 写标记 → 写 state（新补丁成为下次 --lock 的基准）
```

`plan` / `graph` 只读 state，不写标记也不写 state。

### 10.3 完成标记

#### 10.3.1 `version=` 是校验字段

- `marker::site` 读到 `version=` 后和期望补丁比对；不一致按“没有标记”处理
  （项目模式判为 `Interrupted`，可续装）；
- `marker::write` 显式接收版本，不再从内容反推路径；
- `read_marker` 仍要求 `version` 非空。

目录被手工改名、全局 prefix 换过补丁但标记没换时，不会把错的树当成“装完”。

#### 10.3.2 `resolution=`

```text
version=8.5.12
scheme=...
resolution=...
scheme_cmd=...
```

`scheme` 指纹包含组件选择、版本绑定和构建目标；`--targets` 或配置变化会改变指纹。
`install` 判定“配置一致”时同时比较 `scheme` 和 `resolution`：

- 两个都相同：一致；
- scheme 相同、resolution 不同：提示“编译配置一致，但解析出的版本与本次不同”，
  用 `--force` 或显式补丁重装；
- 旧标记没有 `resolution`：只比较 scheme。

### 10.4 state schema

不改 schema 版本，仍是 version 3；现有 state 不需要迁移。字段语义变化：

- `components.<名字>.version` / `deps.<名字>.version` 是 `--lock` 的精确版本来源；
- `bindings` 仍然只保存用户显式钉的线，不把 state 的 line 写进去；
- `targets` 是新增可选字段，参与方案指纹和 `scheme --lock` 的 `--targets` 导出，
  不参与版本选线；
- `tools` 是新增可选字段（按版本列），只给 `--lock` 提供 built 工具版本；不参与
  方案指纹和 `resolution=`。

## 11. 测试

### 11.1 集成测试 `tests/plan_lock.rs`

夹具：复制 `recipes/` 到临时目录，写临时 state，把 catalog 改成“已升级但保留旧补丁”，
调用真实二进制 `plan` / `scheme`，只断言 `order[].version` 和输出，不下载不编译。

| # | 测试 | 断言 |
| --- | --- | --- |
| 1 | `default_keeps_product_patch_but_takes_catalog_dep_patches` | 无 `--lock`：产品仍是 8.5.12，dep 取 3.5.10 / 1.3.3 |
| 2 | `lock_uses_the_patches_recorded_in_state` | `--lock`：产品 8.5.12，dep 回到 3.5.9 / 1.3.2 |
| 3 | `lock_never_falls_back_when_the_old_patch_was_removed` | 删掉 zlib 1.3.2 后 `--lock` 失败，stderr 含 `zlib 1.3.2`，且不产出 1.3.3 |
| 4 | `lock_requires_an_installed_version` | 没有 `installed_version` 时报 `--lock 没有可锁定的版本` |
| 5 | `no_state_means_no_lock_source` | 没有 state 时同样报错 |
| 6 | `cli_line_override_replaces_the_locked_binding` | state 3.5/3.5.9 + `--dep openssl=4.0 --lock` → line 4.0、版本 4.0.0 |
| 7 | `lock_ignores_state_deps_outside_the_current_closure` | 闭包外的 zlib 记录不参与 pin；旧补丁缺失也不影响 |
| 8 | `lock_does_not_pin_deps_from_removed_components` | 已删除组件留下的 libxml2 2.15.0 不参与 pin，catalog 没有也不报错 |
| 9 | `lock_uses_the_versions_written_by_the_last_update` | update 后的新补丁（8.5.13 / 3.5.10 / 1.3.3）成为下次 `--lock` 基准 |
| 10 | `product_patch_without_state_pin_reports_a_catalog_error` | 默认模式删掉 8.5.12 时报 `state 里的 php 8.5.12`，不回落 8.5.13 |
| 11 | `scheme_lock_exports_product_and_exact_patches` | 输出 `phprt install php@8.5.12`、`--dep openssl=3.5.9`、`--dep zlib=1.3.2`；不含 8.5.13 和 `version:8.5.12` |
| 12 | `scheme_lock_refuses_a_patch_missing_from_catalog` | 旧补丁缺失时报错，不生成命令 |
| 13 | `remove_prunes_bindings_outside_the_closure` | remove 后 state.bindings 只剩闭包内节点，libmemcached 被清掉 |
| 14 | `stale_binding_does_not_block_later_commands` | remove 收窄后再删旧 catalog 线，plan / add / remove / update 的 resolver 不再被 stale binding 挡住 |
| 15 | `scheme_lock_skips_bindings_outside_the_closure` | 手写进 state 的闭包外 binding 不出现在 `scheme --lock` 命令里 |
| 16 | `missing_stale_line_does_not_block_remove` | catalog 删掉旧线后，stale binding 不阻断执行 remove 本身 |
| 17 | `scheme_lock_round_trips_the_resolved_graph` | 源 state 删掉闭包内 zlib 的 version 记录后，`scheme --lock` 用本次解析的 artifact 补齐；导出的命令在全新 root 里跑出的节点集合、version、line、mode、features、依赖边与源 state 完全一致 |
| 18 | `second_pass_line_change_is_checked` | 加一条按 line 改变 `needs` 的 recipe 线：`--lock` 报“依赖闭包变了”并失败，普通命令只警告 |

`update` 本身要编译，e2e 不跑它；第 9 条用“update 之后的 state”验证基准传递，
`select_product_patch` 的单测覆盖 update 取该线最大补丁。

### 11.2 单元测试

`src/lock.rs`：

- 只锁当前闭包；闭包外的组件和历史依赖不产生 pin；
- 已删除组件的 `bindings` 残留不产生 pin；
- 不带 `--lock` 只生成 line，不生成 version；
- bundled 组件的 `version` 不进 pin，同名依赖仍锁；
- CLI `Line` / `Version` 都整体覆盖旧 binding；
- update 之后的 state 是下次 lock 的基准；
- CLI 值先按线匹配、再按补丁反查线；
- `exact_versions` 只导出闭包内节点，闭包外旧补丁缺失不报错；
- 闭包里 state 没记录的节点用本次解析的 artifact version 补齐，不允许整条 pin 漏掉；
- 补齐后同样要过 catalog 校验，state 旧版本缺失时报错，不回落最大版本。

`src/plan.rs`：

- `inputs` 记录每个节点的 `children()` 输入快照，供两遍解析之间比较。

`src/main.rs`：

- `check_closure_change` 比较两遍解析的闭包和 plan 输入；`--lock` 不一致时报错，
  普通命令只警告；
- `uses_resolver` 对 `scheme --lock` 生效，让导出命令前先走完整两遍解析。

`src/marker.rs`：

- `resolution_digest` 与节点顺序无关，版本变化会改变结果；
- 标记 `version=` 与期望补丁不一致 → `Interrupted`；
- scheme 相同、resolution 不同 → 拒绝并说明“解析出的版本与本次不同”；
- 标记读写 roundtrip。

`src/state.rs`：

- `prune_deps` 保留 line / version，只删闭包外的 dep、收窄 feature；
- `prune_bindings` 只保留最终闭包里的名字。

### 11.3 手工验收

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets

cargo run -- plan php@8.5 --json
cargo run -- plan php@8.5 --lock --json
cargo run -- scheme php@8.5
cargo run -- scheme php@8.5 --lock
```

真实项目上没有 catalog 变化时，两次 `plan` 的版本应该相同；集成测试负责模拟 catalog
升级后的差异。

## 12. 已定决策

- 默认把产品补丁钉在 `installed_version`，升级只走 `update` 或显式补丁。
- 默认把已有 line 钉在 state，bundle 默认线只影响新节点。
- `--lock` 只锁“当前闭包 ∩ state”里已有的补丁；新节点仍取 catalog 当前。
- CLI 对一个名字的新选择整体替换 line + version，禁止跨线组合。
- `update` 保持 Scheme 和 line，只在线内前进补丁；成功结果成为下次 `--lock` 基准。
- `remove` 算出最终闭包后按闭包收窄 `state.bindings`；闭包外的显式线没有语义意义。
- 第一遍不因 `state.bindings` 里 catalog 已缺失的 line 报错；闭包内的失效线在第二遍报错。
- `version:` / `--dep` 复用现有语法，值可以是线或具体补丁。
- `scheme --lock` 的版本来源：state 记录优先，闭包里 state 没记录的节点用本次解析
  的 artifact version 补齐，不允许整条 pin 漏掉。
- 两遍解析结束后做闭包校验：节点集合变了，`--lock` 报错、普通命令警告；集合没变
  但 plan 输入变了只警告。
- `install` 也支持 `--lock`，保证和 `plan` / `add` / `remove` 一致。
- 完整方案列表和 `--lock` 冲突。
- 不新增独立 lock 文件；state 是锁的来源。
- state schema 保持 version 3。
- 完成标记保留 `version=`，作为和期望补丁比对的校验字段。
- 完成标记只新增 `resolution=` 哈希；不存 line、不存依赖版本，也不作为解析来源。
- `resolution` 用固定节点排序、固定字段顺序、固定文本格式后再 SHA-256。

## 13. 仍存在的边界

- 两遍解析不迭代到不动点。`version` 不参与闭包（见第 3.2 节的 Recipe 契约），所以当前
  两遍足够；如果以后有 recipe 按 version 改变 `needs` / `features` / `tools`，第二遍
  之后需要再算一轮，当前实现不会自动做。闭包变化会被第 3.2 节第 3 步拦住（`--lock`
  报错、普通命令警告）；节点集合没变但 plan 输入变了的情况只警告，不保证整个 plan
  等价。
- `--lock` 只锁版本，不锁构建参数、feature 并集、宿主机工具版本。
- `remove` 会按最终闭包收窄 `state.bindings`；更早版本留下或手工写入的闭包外 binding
  仍可能留在 state 里，解析时会被闭包过滤，但没有 `remove` 就不会自动清掉。
- 一个系列多棵安装树时，`state.installed_version` 决定“哪棵树是当前基准”；
  其余树的 marker 只用于该树自己的重复安装判定。
