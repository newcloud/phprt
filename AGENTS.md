# phprtv3 开发约定

## 1. 文档必须和代码同步

文档入口是 [docs/DESIGN.md](docs/DESIGN.md)，它按状态链接所有子文档。README 只放使用
示例，设计细节写进 `docs/`。

目录按状态分三层，新文档不要放错：

| 目录 | 内容 | 规则 |
| --- | --- | --- |
| `docs/` | 已实现的行为 | 必须与代码一致，能被直接复制执行 |
| `docs/plan/` | 未实现的方案 | 文首写「状态：方案，未实现」；现行文档不得把它当作已实现引用 |
| `docs/notes/` | 一次性验证记录、设计走查 | 只说明当时怎么验证、为什么这么定，不作实施依据 |

- **新增功能**：改主文档的模块/决策/索引，再写对应子文档；新子文档必须在
  `docs/DESIGN.md` 的索引表里加链接。
- 方案落地或被放弃时，同步移动/删除文档：落地后从 `plan/` 移到 `docs/` 并改状态；
  放弃后直接删，不保留「已废弃」说明。
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
| 上游开关盘点、feature 取舍、默认值判据 | `docs/FEATURES.md` |
| 子命令、`产品@系列`、`--component`、`--dep`、错误 | `docs/CLI.md` |
| 选线、依赖线交集、依赖边、子功能传播 | `docs/SOLVER.md` |
| 文本/JSON 输出、路径、字段 | `docs/OUTPUT.md` |
| state.json、bindings、line/version | `docs/STATE.md` |
| 默认版本复用、`--lock`、`scheme --lock`、catalog 旧版本保留 | `docs/LOCK.md` |
| 下载、缓存、镜像、分片续传、进度 UI | `docs/DOWNLOAD.md` |
| 解压顺序、目录、格式、跳过与安全 | `docs/EXTRACT.md` |
| 构建顺序、环境、签名跳过、日志、rpath、ini、入口 | `docs/BUILD.md` |
| 产品共享 profile、工具最低版本、安装后动作、入口模板 | `docs/PRODUCT.md` |
| 宿主缺命令的安装提示、包名映射、发行版识别 | `docs/HOST-COMMANDS.md` |
| 构建环境（`buildimages`）：镜像缓存、实例、锁、自动装包 | `docs/BUILD-IMAGES.md` |
| 构建期工具与编译器档位 | `docs/TOOLS.md` |
| 运行镜像（未实现） | `docs/plan/RUNTIME-IMAGE.md` + `docs/DESIGN.md` 索引 |
| 模块职责、数据流、关键决策、目录 | `docs/DESIGN.md` |

## 2. 不开发没明确要求的使用方式

与 `/home/z/AGENTS.md` 的开发原则一致：只实现明确说出的用法、参数、默认值和行为；
推荐先讨论，用户确认后再写。已实现但属于自作主张的部分要撤掉。

## 3. 不要确认显而易见的事

- 组件对必需依赖是事实，不是可选项：curl 一定依赖 libcurl，openssl 组件一定依赖
  dep openssl。直接按契约写进 recipe，不要拿这类问题让用户确认。
- 同一能力有多种等价写法时，直接选一种定下来、写进契约和文档，不要把选项抛回给用户。
- 只对真正有歧义、会影响外部行为的决策提问；能从领域常识或现有契约直接推出的，
  自己定，并在实现里体现。

## 4. 版本数据规则

- 版本声明原则（详见 `docs/ADD-RECIPE.md` §2）：只声明“我们主动适配”的线。
  默认用最新、ABI 兼容的依赖不逐系列重复声明；不写就是沿用 bundle 默认 / 唯一可用线。
  例外（某系列与某线不兼容）必须写进约束并注明原因。主线默认只写一处（bundle 的
  `[deps]`），组件约束只写允许/不允许的事实。
- 新增版本线的判据：产品需要、有必要主动适配最新、产品支持、有特殊功能或性能收益；
  四条都不成立就不加。“能编过”不等于“值得加”，也不追每个上游版本；加线要跑 build
  验收并在注释里写清适配理由。
- agent 不要过度使用“省略即默认”这条：
  - 已知的不兼容、构建失败、行为差异不能省；
  - 不能为了“可配置”新增未验证的线（每条线都要有 catalog 地址并通过构建）；
  - 不要把同一个默认线在每个系列里重复一遍，也不要靠约束恰好剩一条来隐式选线；
    看到 `自动使用唯一可用线` 时，要么在 bundle 写明确默认，要么把约束收敛成只写例外。
- 补丁号只写在 `recipes/catalog.toml`；recipe、bundle 不写补丁号。state 只记录实际装过的
  `line` / `version`，不复制 `url` / `sha256`；解析时 `--lock` 从 state 取版本、从 catalog
  取源码，详见 `docs/LOCK.md`。
- bundle 顶层只写有独立版本线的组件；依赖库默认线写 `[deps]`。bundled 组件没有版本线，不能写顶层。
- catalog 上的注释不删除。改地址、改版本时同步更新注释内容，保留原意。
- catalog 的一条线是一个补丁数组：默认取数组中版本最大的补丁，和顺序无关。
  新补丁直接加进数组，旧补丁保留不删除。产品、组件、依赖库的所有线都适用；
  显式安装旧版本、`--lock`、`scheme --lock` 按版本从数组里取源码，缺失时报错，
  而不是改用最大版本。
- 填 catalog 前先按 `docs/ADD-RECIPE.md` 判断下载地址是否跳转：跨站点跳转写跳转后的
  最终地址并注释说明；同一发布方的临时签名跳转写稳定发布地址。
- dep recipe 不写产品关系；产品与库版本的对应写在消费组件或产品里。
- 有自己版本线的组件用 `LINES`；bundled 组件用 `PRODUCT_LINES` 约束同名依赖库，
  用 `PRODUCT_DEP_LINES` 约束不同名依赖（如 gd 的 libpng/jpeg）。
- `target(ctx)` 只写构建参数，不判断兼容性。
- 是否 bundled、bundled 组件的关闭参数写在 `products/php/<系列>.star` 的
  `bundled` / `bundled_disable`；component recipe 不写 `origin`、`static_enable`、
  `static_disable`，用 `ctx["is_bundled"]`。
- 上游源码要先改再编时（如删除已移除头文件的 include），或需要把只有 `config0.m4`
  的扩展准备成 `config.m4` 时，component recipe 用
  `prepare = [...]` 写 shell 命令：standalone 构建之前执行、必须幂等、会进构建签名；
  能用 configure 参数解决的事不要写 prepare；通用构建执行器不按文件名自动处理。
- 默认启用集合写在 bundle 的 `[default_components]` 表：`mode` / `features` / `deps`，
  不允许 `version:`；有依赖的 bundled 组件必须一起写，不做依赖展开。
- 产品构建目标（如 PHP 的 cli / fpm / cgi）用 `product(targets = {...})` 声明，
  默认值写在 bundle 的 `[targets] default`。targets 必须逐系列写完整，不放进
  `_profile.star`，也不做共享常量或 `extend_targets`；版本差异直接写在该系列文件。
- `disable_flags` 只能写该版本上游真实支持的禁用参数；必须查 `configure --help` /
  `configure.ac`，不能按 `--enable-*` 推断。没有就留空，未选中时不生成参数。
- target 的 `config` 类型只支持 `string` / `list`，模板必须含 `{value}`；未知 target、
  未知配置项、类型不匹配、重复项都必须报错，不静默忽略。
- 组件被选中时的默认 feature 写在 `[component_presets]`；`[default_components]` 用
  `ref = "component_presets.<同名组件>"` 引用，写 ref 时不再重复 features/deps。
- 新增或修改 component / dep recipe 时，必须按 `docs/FEATURES.md` 盘点上游选项：
  跑 `phprt audit`，把每个选项处置成 `configure` / `features` / `ignored`（含理由），
  默认值按该文档的四条判据写进 bundle；`audit` 不通过不算做完。
- 常用 / 有需求的开关要开发，不能一律记成 `ignored`：需要新库就加 dep recipe，
  用 feature 的 `needs` 让它只在选中时引入。`ignored` 只留给少用、调试和缺依赖未排期的项。
- 组件自己的条件能力用 `requires` 声明，不写成 `target(ctx)` 里的 if：
  `feature(configure=[...], requires={"product": ["8.5","8.6"], "dep": {"openssl": ["3.5","4.0"]}})`。
  显式选择不满足时报错并说明是哪一条不满足；bundle 默认不满足时提示并丢弃。
- 新增/删除版本线时同步改 catalog、组件 recipe 的
  `LINES`/`PRODUCT_LINES`/`PRODUCT_DEP_LINES`，必要时改 bundle。
- 每个产品系列必须有对应的 `bundles/<产品>/<系列>.toml` 和
  `products/<产品>/<系列>.star`。
- 产品共享 profile 写在 `products/<产品>/_profile.star`，只定义 `PROFILE`；
  系列 recipe 用 `profile = PROFILE`，差异用 `extend_profile(PROFILE, ...)` 覆盖
  （tools 按名字合并、env 按 key 合并，其他字段传了才替换）。
- bundle 的 `[deps]` 和 CLI 的 `--dep` 都是硬要求：与已选组件的版本线约束冲突时
  直接报错，不会自动换线；要换必须显式写 `--dep`，并同步改 bundle，别让默认线
  和组件选择长期不一致。
- 新增 recipe 的硬校验（写错会直接加载失败，不静默忽略）：
  - `Target` 拒绝未知字段；旧的 `host_tools` 已删除，写了会报错；
  - `tools` 里不能写 `gcc` / `g++` / `gxx` / `cc` / `c++`，编译器只属于 `toolchain`；
  - `recipes/tools/<名字>.star`：`catalog.toml` 有同名条目就按版本下载构建；没有
    就是系统提供（如 perl），只读 `feature_checks`，不下载不编译。只能用
    `tool_target(...)`，不能声明 `needs` 或产品 / 组件字段；
  - `toolchain` 的档位不能重叠，`gcc` 档位只写 `cflags`、`gxx` 档位只写 `cxxflags`。
- 构建期工具统一写在 `tools`，可带版本区间：
  `tool("make", min = "4.0")`、`tool("autoconf", min = "2.68")`。
  产品 profile 的 `tools` 是产品 / 依赖 / 组件节点的基线；只有部分依赖需要的命令
  （cmake、nasm）写在该 dep / component 的 `tools`。解析宿主优先，宿主不满足时
  复用 `.phprt/build/tools`，再否则从 `recipes/tools/<名字>.star` 构建；同名工具允许
  多版本并存。gcc / g++ 不写在 `tools` 里，由产品系列 recipe 的
  `toolchain = TOOLCHAIN` 声明档位。详见 `docs/TOOLS.md`。
- 安装后行为写在产品 profile 的 `post_install`：`ensure_dir` / `write_file` /
  `copy_ca_bundle` / `write_component_ini`。不往 Rust 里加产品专用分支，也不使用
  任意 shell；需要改上游源码的命令仍写在组件的 `prepare`。组件 ini 的目录由
  `write_component_ini` 声明，Rust 不写死 `etc/conf.d`。
- 入口模板放在 `products/<产品>/` 下，用 `entry_template = "entry.sh"` 引用。

## 5. 验收

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets
# `.github/workflows/ci.yml` 在每次 push / PR 跑这三条。

cargo run -- audit php@8.5                        # 上游开关必须全部处置
cargo run -- install php@8.5                      # 默认组件 + PHP
cargo run -- install php@8.5 --disable-default-component openssl --disable-default-component curl --disable-default-component libxml  # 只装 PHP
cargo run -- add php@8.5 swoole
cargo run -- add php@8.5 xdebug
cargo run -- install php@8.5
cargo run -- remove php@8.5 xdebug
cargo run -- install php@8.5 --component 'curl=mode:shared,dep:libcurl[ssl,nghttp2]'
cargo run -- install php@8.5 --component gd
cargo run -- install php@8.5 --component 'gd=dep:jpeg[simd]'
cargo run -- install php@8.5 --component 'gd=feature:[jpeg,webp,freetype,avif]'
cargo run -- install php@8.6 --component 'gd=feature:[jpeg,webp,freetype,tiff,heif,imagequant,uhdr]'  # 8.6 的 gd 新后端
cargo run -- install php@7.4 --component swoole # swoole 4.8 + openssl 1.1.1
cargo run -- install php@8.2 --component swoole=version:6.0
cargo run -- install php@8.5 --component swoole --dep openssl=4.0
cargo run -- install php@8.5 --component swoole=version:4.8 # 必须失败
cargo run -- install                            # 必须提示指定产品
cargo run -- install php@8.5 nginx@1.29         # 必须失败：一次只能一个产品
cargo run -- list
cargo run -- list php@8.5 --installed
cargo run -- list php@8.5 --pending
cargo run -- plan php@8.5 --json
cargo run -- update php@8.5

cargo run -- install php@8.5 --downloads                       # 只下载，不写 state
cargo run -- install php@8.5 --component swoole --downloads
cargo run -- install php@8.5 --downloads --offline             # 缓存空时必须失败
cargo run -- install php@8.5 --component swoole --offline      # 下载+解压+编译+写 state

cargo run -- install nginx@1.29
./.phprt/nginx-1.29.8/sbin/nginx -V
cargo run -- add nginx@1.29 lua-nginx-module
cargo run -- install nginx@1.29
ls .phprt/nginx-1.29.8/modules/

./myphp-8.5 -v
./myphp-8.5 -m | grep -E 'openssl|curl|swoole|xdebug'
```

测试和文档都对上才算改完。
