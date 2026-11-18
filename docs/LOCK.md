# 锁定与旧补丁（实施文档）

状态：待实施。本文定义 `--lock`、`scheme --lock`、默认版本复用和 catalog 旧补丁保留规则；
实施后同步到 CLI / STATE / FLOW / DESIGN / BUILD / OUTPUT / README。

## 1. 问题

`state.json` 已经记录了每个组件和依赖实际使用的 `line` / `version`，但解析时没有使用：

```text
catalog 里 openssl 3.5.9 -> 3.5.10
  → bind::resolve 取 catalog 当前补丁 3.5.10
  → dep 构建签名变化（build.rs）
  → openssl 重编
  → 产品构建签名变化
  → PHP 跟着重编
  → state.deps.version 被新版本覆盖
```

`add` / `remove` 还会重新解析产品补丁：目标写 `php@8.5` 时总是取 catalog 当前补丁，
检查安装树却用 `installed_version`。catalog 升到 8.5.13 后，`add` 会装出一棵新的
8.5.13 树，旧树保留，`installed_version` 被改掉。

其他相关问题：

- `pins` 只来自 `bindings + CLI`，bundle 默认线每次重算；bundle 改默认线后，已有项目
  的 `update` 也会换线，和 `STATE.md` 的“update 只在线内换补丁”不一致。
- `remove` 不重编（shared / dynamic）时，也会用当前 catalog 的结果重写 `state.deps`，
  记下并没有真正编译过的版本。
- 完成标记里的 scheme 指纹不含版本，同一方案、不同补丁会被判成“配置一致”。
- `scheme` 只导出用户显式钉的版本线，产品补丁、实际解析出的线、依赖补丁都不导出。

## 2. 目标

分两层解决：

| 模式 | 锁定内容 | 补丁 |
| --- | --- | --- |
| 默认（不带 `--lock`） | 已有组件的 line、已有依赖的 line、产品补丁用 `installed_version` | 仍取 catalog 当前补丁 |
| `--lock` | 在默认基础上，把已有节点锁到 `state.*.version` | 不跟随 catalog 升级；缺失就报错 |
| `update` | 产品补丁取 catalog 最新；已有 line 不变 | 在线内取最新补丁 |

另外：

- `scheme --lock` 导出产品补丁、实际 line 和精确补丁，供换机器复现；
- 不新增独立 lock 文件，state 仍是唯一来源；
- `plan` 必须能直接验证默认解析和 `--lock` 解析的版本差异；
- catalog 默认保留旧补丁，作为 `--lock` 和显式旧补丁安装的源码来源。

不在本次范围：

- 独立 `phprt.lock` 文件；
- 把 `url` / `sha256` 复制进 state；
- 编译器、recipe、环境变量的逐字节可复现。

## 3. 默认行为

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

### 3.1 记录缺失

默认模式下，state 记录的 line 或产品补丁在 catalog 里不存在时直接报错，不回落最新版本：

```text
state 里的 php 8.5.12 不在 catalog（8.5 当前 8.5.13，无旧补丁）
→ 用 phprt update php@8.5 升级，或 install 一个 catalog 里有的补丁
```

### 3.2 完整方案列表

`install <产品>@<系列> '组件列表'` 会替换当前组件方案：

- 不再使用旧 `state.components` / `state.deps` 的 line 作为 pin；
- 列表里的 `version:` / `--dep` 和 bundle 默认线决定新方案；
- 产品补丁仍按第 3 节处理（已有 `installed_version` 时保持）。

## 4. `--lock`

`--lock` 在默认行为上再锁解析结果。

### 4.1 支持的命名

```bash
phprt plan    php@8.5 --lock [--json]
phprt install php@8.5 --lock
phprt add     php@8.5 swoole --lock
phprt remove  php@8.5 swoole --lock
phprt scheme  php@8.5 --lock
```

`update` 不支持 `--lock`；它就是升级入口。

### 4.2 规则

1. 必须有 `installed_version`；没有安装完成的产品时报错：

   ```text
   当前没有安装完成的产品，--lock 没有可锁定的版本；
   先 phprt install php@8.5，或用 scheme --lock 导出源项目的精确方案
   ```

2. 产品补丁、已有外部组件补丁、已有依赖补丁都取 state 里的 `version`。
3. 新加入、state 里没有记录的节点仍取 catalog 当前补丁；`--lock` 只锁已有记录。
4. catalog 当前线或 `patches` 里找不到锁定补丁时报错，不回落：

   ```text
   lock 里的 zlib 1.3.2 不在 catalog（线 1.3 当前 1.3.3，无旧补丁）；
   用 phprt update php@8.5 升级，或在 catalog 里保留旧补丁
   ```

5. CLI 显式选择覆盖 lock：
   - `--dep openssl=4.0`：openssl 改用 4.0 线，丢掉旧的 3.5 补丁 pin，取 4.0 当前补丁；
   - `version:6.2.5`：按 catalog 反查线并锁定 6.2.5；
   - 显式产品补丁：产品按显式补丁，其他已有节点仍按 lock。
6. `--lock` 不改变构建必要性。static / fold 组件改 PHP configure 时，PHP 仍会重编；
   `--lock` 只避免已有依赖被升级而多出的重编。

### 4.3 和完整方案列表冲突

`--lock` 和完整方案列表不能一起用：

```bash
phprt install php@8.5 --lock 'curl=mode:static,...'   # 报错
```

方案列表已经可以带精确版本；两个来源同时决定版本会互相覆盖。换机器复现用
`scheme --lock` 生成的命令。

## 5. `scheme --lock`

普通 `scheme` 保持现有契约：只导出方案，不导出补丁。

`scheme --lock` 导出当时的解析结果：

```bash
phprt install php@8.5.12 \
  'openssl=mode:static,feature:[argon2],curl=mode:static,dep:libcurl[ssl],swoole=mode:shared,version:6.2.5,zlib=mode:static' \
  --dep libcurl=8.23.0 --dep openssl=3.5.9 --dep zlib=1.3.2
```

导出内容：

| 对象 | 导出方式 |
| --- | --- |
| 产品 | 目标写成 `产品@补丁` |
| 有自己版本线的外部组件 | `version:<线或补丁>`；有补丁时写补丁 |
| bundled 组件 | 不写 `version:`，源码在产品 tarball 里 |
| 依赖库 | `--dep 名字=<线或补丁>`；有补丁时写补丁 |
| mode / feature / dep 转授 | 和普通 `scheme` 一样 |

`version:` 和 `--dep` 的值接受线或具体补丁：

1. 先按 catalog 的 line 精确匹配；
2. 不再匹配时，在当前补丁和 `patches` 里按版本号查找所属 line；
3. 都找不到就报错，列出可用线和补丁。

`scheme --lock` 要求 state 有 `installed_version`，否则报错。导出的每个旧补丁都必须能在
catalog 里找到；找不到时直接报错，不生成一条执行会失败的假 lock。

## 6. catalog 默认保留旧补丁

这是本次改动的默认维护规则，适用于产品、组件和依赖库的所有版本线。

升级某条线的补丁时，把原来的 `version` / `url` / `sha256` 移进 `patches`，再写新补丁：

```toml
[zlib]
"1.3" = { version = "1.3.3", url = "https://zlib.net/fossils/zlib-1.3.3.tar.gz", sha256 = "...", patches = [
    { version = "1.3.2", url = "https://zlib.net/fossils/zlib-1.3.2.tar.gz", sha256 = "..." },
] }
```

规则：

- `version` / `url` / `sha256` 永远是当前最新补丁；
- 旧补丁默认不删除，按发布时间倒序或升级顺序追加；
- `patches` 里的 `url` / `sha256` 必须保留旧补丁的真实值，不能只写版本号；
- 显式安装旧补丁、`--lock`、`scheme --lock` 都只从这里取源码；
- state 只记版本，不复制 `url` / `sha256`；catalog 是唯一源码来源；
- 旧补丁缺失时命令直接报错，不自动改用当前补丁。

## 7. 实现落点

### 7.1 effective pins

从 state 生成一份解析输入，不改 state schema：

```text
lines:    名字 -> 版本线
versions: 名字 -> 精确补丁
product:  产品补丁
```

- `lines`：`state.deps[].line` + 外部组件 `state.components[].line`；
  bundled 组件的 line 跳过。
- `versions`：`state.deps[].version` + 外部组件 `state.components[].version`；
  只在 `--lock` 时启用。
- `product`：`installed_version`；默认和 `--lock` 都使用，`update` 用 catalog 最新。
- CLI 显式选择在生成 pins 之后覆盖，并丢掉同名旧 pin。

`bindings` 仍然只保存用户显式钉的线，不把 state 的 line 写进去；这样普通 `scheme`
的语义不变。

### 7.2 bind::resolve

`bind::resolve` 增加 `versions: BTreeMap<String, String>`：

- 选好 line 后，如果该名字有 version pin，用
  `catalog.artifact_for(name, line, version)` 取补丁；
- 找不到时报错，列出该线当前补丁和 `patches`；
- `Bindings` 增加解析出的精确版本，供 recipe 的 `self.version` 和构建签名使用。

### 7.3 产品补丁

产品补丁在加载 recipe 前确定：

```text
显式 target 补丁 > (--lock 或默认：installed_version) > update：catalog 当前
```

仍然通过 `catalog.artifact_for(product, series, version)` 注入 url / sha256。

### 7.4 `scheme --lock`

需要知道哪些组件是 bundled：

- `scheme_command` 增加精确版本和 bundled 判断；
- 外部组件导出 `version:`，bundled 组件不导出；
- 依赖按 `state.deps` 导出 `--dep`；
- 普通 `scheme` 继续只导出 `bindings` 里的显式线。

### 7.5 完成标记

#### 7.5.1 `version=` 改成校验字段

问题：`version=` 现在没有任何读取者。`decide` 只比较 `state.installed_version` 和目标补丁；
项目模式的补丁在目录名里，全局模式的权威值在 `<prefix>/.phprt-state.json`。
它只剩一个隐性用途：`remove` 不重编时重写标记，`marker::write` 用内容里的版本反推文件路径。

修改：

- 保留该行，`marker::site` 读到后和期望版本比对；不一致按“没有标记”处理，
  沿用“宁可多装一次”的规则（项目模式判为 `Interrupted`，可续装）；
- `marker::write` 改为显式接收版本，不再从内容反推路径；
- `read_marker` 仍要求 `version` 非空。

目标：把死字段变成防错字段。目录被手工改名、全局 prefix 换过补丁但标记没换时，
不会把错的树当成“装完”。

#### 7.5.2 `resolution=`

`.phprt-installed` 增加一行 `resolution=`：

```text
version=8.5.12
scheme=...
resolution=...
scheme_cmd=...
```

`resolution` 是产品版本、每个构建节点的 `line` / `version` / `sha256` 的 sha256。
`install` 判定“配置一致”时同时比较 `scheme` 和 `resolution`：

- 两个都相同：一致；
- scheme 相同、resolution 不同：不一致，提示版本不同，用 `--force` 或显式补丁；
- 旧标记没有 `resolution`：只比较 scheme，并在提示里说明。

边界：`resolution` 只用于比较，不用于解析。marker 里不存 line、不存依赖版本；
版本来源仍然只有 `state`，否则一个系列有多棵树时无法确定该听哪棵。

目标：同一方案、不同补丁不再被判成“配置一致”；同时不把 marker 变成第二个 lock 来源。

#### 7.5.3 顺带纠正“切回旧补丁不用重编”

问题：`STATE.md` §7 和 `FLOW.md` §2.1 原写“切回旧补丁时，旧树的标记还在就不用重编”。
实际不是：`decide` 在目标补丁与 `installed_version` 不同时返回 `Install`，仍会走一遍流水线；
跳过编译靠每个节点工作目录里的 `.phprt-sig` 签名，不靠 marker。

修改：两处改成“重新走流水线，签名一致的节点跳过编译”，并指向 `BUILD.md` §4。

目标：不再让 marker 看起来承担编译缓存职责，避免和新加的 `resolution` 职责混淆。

### 7.6 remove 不重编

`remove` 走 shared / dynamic、不需要重编时：

- 不调用按当前 catalog 重算的 `set_deps`；
- 保留现有 `state.deps` 的 line / version；
- 只按剩余组件收窄 feature 并删掉已不在闭包里的 dep；
- 真正重编时再写入本次解析结果。

### 7.7 state schema

不改 schema 版本，仍是 version 3；现有 state 不需要迁移。

## 8. 测试

### 8.1 plan 集成测试

新增 `tests/plan_lock.rs`，复制 `recipes/` 到临时目录，写临时 state，修改临时
`catalog.toml`，然后调用：

```text
CARGO_BIN_EXE_phprt plan php@8.5 --json
CARGO_BIN_EXE_phprt plan php@8.5 --lock --json
```

不编译，只断言 `order[].version`。

测试夹具：

```text
state:
  php 8.5.12
  components.zlib = static
  deps.zlib = 1.3 / 1.3.2

catalog:
  zlib 1.3 当前 1.3.3
  patches 里保留 1.3.2
```

断言：

1. `plan` 不带 lock：`dep:zlib` 是 `1.3.3`；`product:php@8.5.12`。
2. `plan --lock`：`dep:zlib` 是 `1.3.2`；`product:php@8.5.12`。
3. 删掉 catalog 里的 1.3.2：`plan --lock` 退出非 0，stderr 含 `1.3.2`。
4. 没有 state：`plan --lock` 退出非 0。
5. `plan --lock --component gd=mode:static`：已有 zlib 仍是 1.3.2；gd 新引入的依赖取
   catalog 当前补丁。
6. bundle 默认 openssl 从 3.5 改 4.0，state 里是 3.5：`plan` 仍用 3.5；
   `plan --dep openssl=4.0` 才用 4.0。
7. catalog 把 php 8.5 升到 8.5.13、保留 8.5.12 patches：`plan` 仍是
   `product:php@8.5.12`；`update` 才用 8.5.13。

### 8.2 scheme 导出测试

临时 state 里放一个外部组件：

```text
swoole: line 6.2 / version 6.2.5 / mode shared
```

断言：

```text
phprt scheme php@8.5 --lock
  包含 php@8.5.12
  包含 version:6.2.5
  包含 --dep zlib=1.3.2
  不包含 php@8.5.13
```

普通 `scheme` 的断言保持现有测试不变。

### 8.3 单元测试

- `bind::resolve`：有 / 无 version pin 的补丁选择；
- version pin 不在 catalog 时返回错误；
- effective pins 跳过 bundled 组件；
- CLI 显式 line 覆盖 state 的 line 和补丁；
- `marker::resolution_digest` 随版本变化；
- `marker::site`：标记里的 `version` 和期望版本不一致时按 `Interrupted` 处理；
- `remove` 不重编时 state.deps 版本不变。

### 8.4 手工验收

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

## 9. 实施顺序

1. 先落 catalog 旧补丁保留规则（AGENTS、ADD-RECIPE、README）；
2. `EffectivePins` + `bind::resolve` 的 version pin；
3. 产品补丁选择；
4. `plan` / `install` / `add` / `remove` 的 `--lock`；
5. `scheme --lock`；
6. 完成标记：`version=` 校验和 `resolution=`；
7. `remove` 不重编的 state 写入；
8. `tests/plan_lock.rs` 和单元测试；
9. 同步 CLI / STATE / FLOW / DESIGN / BUILD / OUTPUT / README。

## 10. 已定决策

- 默认把产品补丁钉在 `installed_version`，升级只走 `update` 或显式补丁。
- 默认把已有 line 钉在 state，bundle 默认线只影响新节点。
- `--lock` 只锁 state 里已有的补丁；新节点仍取 catalog 当前。
- `version:` / `--dep` 复用现有语法，值可以是线或具体补丁。
- `install` 也支持 `--lock`，保证和 `plan` / `add` / `remove` 一致。
- 完整方案列表和 `--lock` 冲突。
- 不新增独立 lock 文件；state 是锁的来源。
- state schema 保持 version 3。
- 完成标记保留 `version=`，作为和期望补丁比对的校验字段。
- 完成标记只新增 `resolution=` 哈希；不存 line、不存依赖版本，也不作为解析来源。
