# phprtv3 设计文档

这是设计文档的主入口。子文档按主题拆分，改代码时同步改对应文档；删除功能时
把对应文档一起删掉，不留过时说明。

## 1. 定位与边界

phprtv3 是最小原型，只验证两件事：

1. **Starlark 只做声明**：recipe 描述目标、依赖、源码、编译参数和 ini，
   不执行命令、不访问网络、不读写文件。
2. **Rust 只做求解**：解析 recipe、校验契约、算依赖闭包和拓扑序、展开路径与参数，
   输出可读计划和 JSON。

当前不下载、不编译、不创建安装树。执行器、签名、`verify` 都不在范围内。

`plan` / `graph` / `ext list` 只读；`install` / `update` / `ext add` / `ext remove`
是模拟命令，只写 `.phprt/php-<系列>/state.json`。

## 2. 分层

```text
recipes/**.star  --eval-->  Target 图  --Selection-->  Build order  --render-->  文本 / JSON
     数据                       驱动（Rust）                  plan / graph / list
```

- `recipes/` 是数据：`defs.star` 提供构造函数，每个 recipe 顶层定义 `TARGET`。
- `src/` 是驱动：不出现具体包名，只按 `kind`、`link`、`origin`、`features` 等
  通用字段处理。
- 加一个扩展或依赖只改 `recipes/`，驱动不改。

## 3. 模块

| 文件 | 职责 |
| --- | --- |
| `src/main.rs` | clap 子命令、参数解析、模块串接、退出码 |
| `src/recipe.rs` | Starlark 求值、`Target`/`Feature` 数据结构、契约校验、`--ext` 选择解析 |
| `src/plan.rs` | 依赖边、DFS 拓扑排序、成环检测、shared 扩展后置 |
| `src/render.rs` | 占位符展开、`Sim`/`Node`、文本和 JSON 输出、list/graph 文本 |
| `src/state.rs` | `state.json` 的读取、方案与 feature 并集、installed 状态 |
| `recipes/defs.star` | `dep` / `ext` / `php` / `feature` 构造函数 |

依赖：`starlark 0.14`、`clap`、`serde`/`serde_json`、`anyhow`。

Starlark 求值方式：每个 recipe 在独立的临时 `Module` 里先执行 `defs.star`，
再执行自身，最后读取顶层 `TARGET` 转成 JSON 反序列化成 `Target`。所以 recipe
不需要写 `load()`，也不能跨文件共享状态。

## 4. 关键决策

1. **命名空间**：目标是 `kind:name`，依赖是 `dep:openssl`，扩展是 `ext:openssl`，
   PHP 是 `php:8.5.12`。同名 dep 和 ext 不冲突，`needs` 只指向 dep，`exts` 只指向 ext。
2. **静态/动态由契约决定**：ext 的 `link` 是允许集合，`origin` 决定默认值
   （builtin→static，pecl→shared）。请求不在 `link` 里的方式，在解析阶段直接失败。
3. **子配置与 feature 分开**：ext 的 `subs` 是自己的开关，方括号里的 feature 转给
   `subs.<名字>.dep`；具体编译参数与额外依赖写在 dep 的 `features` 里。
4. **失败要早**：契约错误、未知目标、未知子功能都在解析阶段退出，不输出半份计划。
5. **feature 属于 dep**：ext 的 `sub` 是自己的开关，方括号里的 feature 转给 dep；
   dep 是唯一知道 feature 怎么编的地方。
6. **默认只增不减**：`ext add` / `install` / `update` 对 feature 取并集；只有显式
   `ext remove` 才重算并允许收窄。
7. **不实现没要求的行为**：命令行语法、默认值、输出字段只做明确要求的部分。

## 5. 子文档

| 文档 | 内容 |
| --- | --- |
| [CONTRACT.md](CONTRACT.md) | recipe 字段、默认值、校验规则 |
| [CLI.md](CLI.md) | 子命令、`--ext` 语法、错误与退出码 |
| [SOLVER.md](SOLVER.md) | 依赖边、拓扑顺序、静态/动态、子功能传播 |
| [OUTPUT.md](OUTPUT.md) | 文本/JSON 输出、路径规则、字段含义 |
| [STATE.md](STATE.md) | state.json、方案、feature、写入时机 |

## 6. 目录

```text
phprtv3/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── recipe.rs
│   ├── plan.rs
│   ├── render.rs
│   └── state.rs
├── recipes/
│   ├── defs.star
│   ├── php8.5.star
│   ├── dep/{openssl,libcurl,nghttp2}.star
│   └── ext/{openssl,curl,swoole,xdebug}.star
└── docs/
    ├── DESIGN.md
    ├── CONTRACT.md
    ├── CLI.md
    ├── SOLVER.md
    ├── OUTPUT.md
    └── STATE.md
```

## 7. 验收

```bash
cargo test
cargo clippy --all-targets
cargo run -- plan
cargo run -- plan --ext curl=link:shared,sub:curl[nghttp2]
cargo run -- plan --ext xdebug=link:static   # 必须失败
cargo run -- install --ext swoole=sub:curl[ssl]
cargo run -- ext add xdebug
cargo run -- update
```

测试覆盖：默认模式、显式模式、PECL 拒绝 static、子功能传播、未知子功能、
成环、缺失依赖、歧义名、占位符展开。
