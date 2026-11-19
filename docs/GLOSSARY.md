# 名词字典

这份文档只解释词义，不讲流程。流程看 [DESIGN.md](DESIGN.md) 和 [FLOW.md](FLOW.md)；
字段定义看 [CONTRACT.md](CONTRACT.md) 和 [STATE.md](STATE.md)。

## 1. 三层对象

| 词 | 意思 | 例子 |
| --- | --- | --- |
| 产品 | 被构建的成品，装进自己的安装树 | `php`、`nginx` |
| 组件 | 产品的组成部分；PHP 的扩展是组件 | `curl`、`swoole`、`zlib` |
| 依赖库 | 组件或产品需要的外部 C 库，有自己的版本线 | `openssl`、`libcurl`、`zlib` |

同一个名字可以既是组件又是依赖库：`openssl` 组件是 PHP 内置扩展（源码在产品
tarball 里），`dep:openssl` 是要单独下载编译的库。命令里靠前缀区分：
`component:openssl` 和 `dep:openssl`。

**bundled（内置组件）**：产品源码包里自带的组件，如 PHP 的 `zlib`、`mbstring`。
它没有自己的源码版本，`state.components[name].version` 记的是产品补丁。
**external（外部组件）**：有自己源码和版本线的组件，如 `swoole`、`xdebug`。

## 2. 版本三件套

| 词 | 意思 | 谁管 |
| --- | --- | --- |
| 系列 | 产品的主版本，如 `8.5` | 命令行目标 `php@8.5` |
| 版本线（line） | 组件 / 依赖库的版本区间，如 openssl 的 `3.5` | recipe 的 `LINES`、bundle 的默认线、`--dep NAME=LINE` |
| 补丁（patch / version） | 线里的具体版本，如 `3.5.9` | 只来自 catalog |

易混点：

- **line 不是 version**。`openssl=3.5` 是选线；`--dep openssl=3.5.9` 是钉补丁。
  换线时必须丢掉同名的旧补丁，不允许 `line=3.5` + `version=4.0.x`。
- catalog 升级一条线时，旧补丁移进 `patches` 不删除，`--lock` 和 `scheme --lock`
  从那里取源码（见 [LOCK.md](LOCK.md)）。
- `version` 不参与依赖闭包求解：它只影响最终选哪个 artifact（version / url /
  sha256）和构建元数据，不改 `needs` / `features` / `tools`（契约见 [LOCK.md](LOCK.md) §3.2）。

## 3. 挂载方式：mode / fold / standalone

**mode** 是组件怎么挂到产品上，写在方案里，由产品 profile 定义：PHP 有
`static` / `shared`，nginx 有 `builtin` / `dynamic`。mode 声明里的 `strategy` 字段
才决定构建策略，只有两种：

| strategy | 含义 | 加/删它时 |
| --- | --- | --- |
| fold | 组件只贡献 configure 参数和源码，**编进产品本体** | 产品整个重编 |
| standalone | 产品编完后，组件按自己的 `steps` 单独编 | 只编组件，不重编产品 |

所以“静态添加 `zlib`、`mbstring` 当然是一次编译 PHP”：`static` → fold → 产品
configure 参数变了 → PHP 整个重编。`shared` → standalone → 只多编一个扩展。
这就是 `add` / `remove` 里 fold 组件要弹重编警告、standalone 组件不弹的原因。

不要凭产物形态猜 strategy：nginx 的 `dynamic` 产出 `modules/*.so`，但 strategy 仍是
`fold`，加删它要重编 nginx。

## 4. 方案与选择

| 词 | 意思 |
| --- | --- |
| 方案（scheme） | 当前要构建的组件集合 + 每个组件的 mode / feature / dep 转授；存在 `state.components`，是权威来源 |
| 选择（Selection） | 一次解析用的方案视图：CLI 方案列表 / `--component` 与 state、preset 合并后的结果 |
| preset | bundle 里 `[component_presets]` 的默认 mode / feature / dep 转授 |
| 默认组件 | bundle 里 `[default_components]` 的集合；首次安装时铺进方案 |
| feature | 组件自己的编译开关（如 openssl 的 `argon2`）；dep 的 feature 通过 `dep:NAME[feature]` 转授 |
| 完整方案列表 | `install` 的第二个位置参数，**替换**整个方案，不与默认集合并 |

## 5. 闭包与求解

| 词 | 意思 |
| --- | --- |
| 闭包（desired closure） | `plan::resolve` 从产品出发按 `needs` / feature / tools 展开出来的全部节点；只有闭包里的名字才可能成为 pin |
| order | 闭包的拓扑构建顺序；产品在前，standalone 组件在产品之后 |
| bindings | `state.bindings`，用户显式钉过的版本线；不含 bundle 默认线 |
| Effective Pins | 闭包 ∩ state 生成的 `lines` + `versions`，是 resolver 的锁定输入 |
| pin | 对单个节点的锁定：line pin（选线）或 version pin（选补丁） |
| stale binding | 已删除组件或闭包外节点留在 `state.bindings` 里的历史记录；解析时按闭包过滤，`remove` 时按闭包收窄 |
| 两遍解析 | 第一遍按 CLI / bindings / bundle 取线算闭包；第二遍把 state 的 line / version 纳入 pins 重算（见 [LOCK.md](LOCK.md) §3.2） |

## 6. 落盘

| 词 | 意思 |
| --- | --- |
| state.json | `.phprt/<产品>-<系列>/state.json`；方案、每个节点的 line / version、`bindings`、`installed_version` |
| installed_version | 哪棵安装树是当前基准；产品补丁默认复用它 |
| 安装树 | `.phprt/<产品>-<补丁>/`，一个系列多棵时按 `installed_version` 区分 |
| 完成标记 | 安装树里的 `.phprt-installed`；字段 `version` / `scheme` / `resolution` / `scheme_cmd`，用于重复安装判定和指纹比对 |
| resolution digest | 固定节点排序 + line + version + sha256 的哈希；版本变了才会判“解析结果不同” |

## 7. 命令面速查

| 词 | 意思 |
| --- | --- |
| `install` | 解析 + 下载 + 编译 + 写 state；可带完整方案列表 |
| `add` | 把组件加进方案并**立即构建**（不是只改 state） |
| `remove` | 把组件移出方案、删产物、按最终闭包收窄 `bindings` / `deps` |
| `update` | 保持 Scheme 和 line，只把产品补丁升到 catalog 当前 |
| `plan` / `graph` | 只读解析，不写 state |
| `scheme` | 导出已保存方案；`--lock` 附精确补丁（走两遍解析） |
| `--lock` | 把闭包内、state 有记录的节点锁到实际补丁；缺旧补丁报错不回落 |
| `--force` | 跳过重复安装 / 中断判定和重编确认 |
| `--downloads` / `--offline` | 只下载到缓存 / 只用缓存不联网 |
