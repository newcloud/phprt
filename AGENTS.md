# phprtv3 开发约定

## 1. 文档必须和代码同步

文档入口是 [docs/DESIGN.md](docs/DESIGN.md)，它链接所有子文档。README 只放使用示例，
设计细节写进 `docs/`。

- **新增功能**：改主文档的模块/决策/索引，再写对应子文档；新子文档必须在
  `docs/DESIGN.md` 的索引表里加链接。
- **修改功能**：同步更新字段表、示例、错误信息、输出格式，保证文档与
  `cargo run` 的实际输出一致。
- **删除功能**：同时删除 README、主文档、子文档里所有相关段落和示例。
  不保留「已废弃」「以前支持」之类的说明。
- 文档里的命令和输出要能直接复制执行；改了参数或输出标签就改文档。

改动与文档的对应关系：

| 改动 | 要更新的文档 |
| --- | --- |
| recipe 字段、默认值、校验规则 | `docs/CONTRACT.md` |
| 子命令、参数、`--ext` 语法、错误 | `docs/CLI.md` |
| 依赖边、拓扑顺序、模式、子功能传播 | `docs/SOLVER.md` |
| 文本/JSON 输出、路径、字段 | `docs/OUTPUT.md` |
| state.json、ext 方案与 installed | `docs/STATE.md` |
| 模块职责、数据流、关键决策、目录 | `docs/DESIGN.md` |

## 2. 不开发没明确要求的使用方式

与 `/home/z/AGENTS.md` 的开发原则一致：只实现明确说出的用法、参数、默认值和行为；
推荐先讨论，用户确认后再写。已实现但属于自作主张的部分要撤掉。

## 3. 不要确认显而易见的事

- 扩展对必需依赖是事实，不是可选项：curl 一定依赖 libcurl，openssl 扩展一定依赖
  dep openssl。直接按契约写进 recipe，不要拿这类问题让用户确认。
- 只对真正有歧义、会影响外部行为的决策提问；能从领域常识或现有契约直接推出的，
  自己定，并在实现里体现。

## 4. 验收

```bash
cargo fmt
cargo test
cargo clippy --all-targets
cargo run -- plan
cargo run -- plan --ext curl=link:shared,sub:curl[nghttp2]
cargo run -- plan --ext xdebug=link:static   # 必须失败
cargo run -- install --ext swoole=sub:curl[ssl]
cargo run -- ext add xdebug
cargo run -- ext remove xdebug
cargo run -- ext list --installed
cargo run -- update
```

测试和文档都对上才算改完。
