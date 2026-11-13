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
| recipe 字段、ctx、LINES、catalog/bundle | `docs/CONTRACT.md` |
| 新增/更新版本线、补丁号、catalog 下载地址、扩展默认规则 | `docs/ADD-RECIPE.md` |
| 子命令、`--php`、`--ext`、`--dep`、错误 | `docs/CLI.md` |
| 选线、依赖线交集、依赖边、子功能传播 | `docs/SOLVER.md` |
| 文本/JSON 输出、路径、字段 | `docs/OUTPUT.md` |
| state.json、bindings、line/version | `docs/STATE.md` |
| 下载、缓存、镜像、分片续传、进度 UI | `docs/DOWNLOAD.md` |
| 解压顺序、目录、格式、跳过与安全 | `docs/EXTRACT.md` |
| 构建顺序、环境、签名跳过、日志、rpath、ini、入口 | `docs/BUILD.md` |
| 模块职责、数据流、关键决策、目录 | `docs/DESIGN.md` |

## 2. 不开发没明确要求的使用方式

与 `/home/z/AGENTS.md` 的开发原则一致：只实现明确说出的用法、参数、默认值和行为；
推荐先讨论，用户确认后再写。已实现但属于自作主张的部分要撤掉。

## 3. 不要确认显而易见的事

- 扩展对必需依赖是事实，不是可选项：curl 一定依赖 libcurl，openssl 扩展一定依赖
  dep openssl。直接按契约写进 recipe，不要拿这类问题让用户确认。
- 只对真正有歧义、会影响外部行为的决策提问；能从领域常识或现有契约直接推出的，
  自己定，并在实现里体现。

## 4. 版本数据规则

- 补丁号只写在 `recipes/catalog.toml`；recipe、bundle、state 都不锁补丁。
- catalog 上的注释不删除。改地址、改版本时同步更新注释内容，保留原意。
- 填 catalog 前先按 `docs/ADD-RECIPE.md` 判断下载地址是否跳转：跨站点跳转写跳转后的
  最终地址并注释说明；同一发布方的临时签名跳转写稳定发布地址。
- dep recipe 不写 PHP 关系；PHP 与库版本的对应写在消费扩展里。
- 有自己版本线的扩展用 `LINES`；builtin 扩展用 `PHP_LINES` 约束同名依赖库。
- `target(ctx)` 只写构建参数，不判断兼容性。
- 是否内置、内置扩展的关闭参数写在 `php<系列>.star` 的 `builtin_exts` / `builtin_disable`；
  ext recipe 不写 `origin`、`static_enable`、`static_disable`，用 `ctx["is_builtin"]`。
- 默认启用集合写在 bundle 的 `default_exts`，是空格分隔的扩展 spec，不允许 `version:`；
  有依赖的内置扩展必须一起写（如 `"libxml xml"`），不做依赖展开。
- 新增/删除版本线时同步改 catalog、扩展 recipe 的 `LINES`/`PHP_LINES`，必要时改 bundle。
- 每个 PHP 系列必须有对应的 `bundles/php<系列>.toml` 和 `php<系列>.star`。

## 5. 验收

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets

cargo run -- install 8.5                          # 默认 openssl + curl(ssl) + PHP
cargo run -- install 8.5 --disable-default-ext openssl --disable-default-ext curl  # 只装 PHP
cargo run -- install 8.5 --ext swoole --ext xdebug
cargo run -- install 8.5 --ext swoole             # bundle: swoole 6.2
cargo run -- install 7.4 --ext swoole             # swoole 4.8 + openssl 1.1.1
cargo run -- install 8.2 --ext swoole=version:6.0
cargo run -- install 8.5 --ext swoole --dep openssl=4.0
cargo run -- install 8.5 --ext swoole=version:4.8 # 必须失败
cargo run -- install                               # 必须提示指定 PHP
cargo run -- --php 8.5 ext list --installed
cargo run -- --php 8.5 ext add xdebug
cargo run -- --php 8.5 ext remove xdebug
cargo run -- --php 8.5 update

cargo run -- install 8.5 --downloads                          # 只下载，不写 state
cargo run -- install 8.5 --ext swoole --downloads
cargo run -- install 8.5 --downloads --offline                # 缓存空时必须失败
cargo run -- install 8.5 --ext swoole --offline               # 下载+解压+编译+写 state

./myphp -v
./myphp -m | grep -E 'openssl|curl|swoole|xdebug'
```

测试和文档都对上才算改完。
