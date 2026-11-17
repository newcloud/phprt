# RPATH 设置：oxipatch / patchelf 双后端

本文是待实施的方案。实施后 BUILD.md、PRODUCT.md 等按第 8 节的清单同步更新；
文档与代码冲突时以代码实际行为为准。

## 1. 背景

今天安装树的 rpath 靠外部命令设置（`src/build.rs` 的 `patchelf_rpath()`）：

```text
patchelf --set-rpath <rpath> --force-rpath <file>
```

`patchelf` 因此出现在 php / nginx 产品 profile 的 `tools` 清单里，成为宿主机硬依赖。

目标：默认不依赖任何外部命令，且设置结果与 `patchelf --set-rpath --force-rpath`
在加载器可见语义上完全一致；同时保留 patchelf 命令作为可选后端，一行配置即可切换。

## 2. 设计：新增 `crates/rpath`（包名 `phprt-rpath`）

结构对标 `crates/download`：crate 只收「文件 + rpath 字符串 + 后端选择」，
不关心产品是 PHP 还是 nginx；配置解析留在 `src/config.rs`
（对标现有 `Config::github_proxy()` 把 toml 键转成 `phprt_download::GitHubProxy` 的接法）。

```rust
// crates/rpath/src/lib.rs
pub enum Backend { Oxipatch, Patchelf }

pub struct Options {
    pub backend: Backend,
}

/// 等价于 patchelf --set-rpath <rpath> --force-rpath <path>
pub fn set_rpath(path: &Path, rpath: &str, opts: &Options) -> Result<()>
```

两个后端：

- `Oxipatch`：`oxipatch::ElfFile::open` → `set_rpath(rpath, true)` → `write_in_place`。
  依赖 `oxipatch = { version = "0.1", default-features = false }`
  （关 cli feature，不带 clap/serde 进来；只新增 thiserror 一个依赖）。
- `Patchelf`：现在 `patchelf_rpath()` 里的 `Command` 逻辑原样搬进 crate，
  错误文案保持不变。

`src/build.rs` 的 `patchelf_rpath()` 删除，`deploy_libs()` 两处调用改为
`phprt_rpath::set_rpath(&path, rpath, &opts.rpath)`；`build::Options` 加
`pub rpath: phprt_rpath::Backend`，由 `src/main.rs` 从 `Config` 传入。

## 3. 配置：`recipes/phprt.toml` 新增 `rpath_backend`

默认 `oxipatch`。`src/config.rs` 加同名字段（`serde(default)`），
校验规则对齐 `enable_github_proxy_download`：未知值 bail，
文案 `{FILE_NAME}: rpath_backend 只能是 oxipatch 或 patchelf，实际是 {other}`。

文件里的注释原文（实施时按此加）：

```toml
# rpath 设置方式：oxipatch（默认）/ patchelf
#
# 两种方式行为一致，都等价于 patchelf --set-rpath <值> --force-rpath：
# - oxipatch：用内嵌的 oxipatch 库直接改 ELF 文件，不需要装任何外部命令；
# - patchelf：调用系统 patchelf 命令，选择它要先安装
#   （Fedora: sudo dnf install patchelf）。
# 同时保留两个方式：oxipatch 是新库，遇到边界行为问题时，改这一行就能
# 切回久经考验的 patchelf，不用改代码、不用重编 phprt 本体。
rpath_backend = "oxipatch"
```

## 4. 两个后端的行为对齐（验收底线）

| 语义 | Oxipatch | Patchelf 命令 |
| --- | --- | --- |
| 写入标签 | DT_RPATH（force） | DT_RPATH（--force-rpath） |
| 旧的 DT_RPATH / DT_RUNPATH | 先删再写 | 先删再写 |
| 新串比旧槽长 | 文件增长（迁移 .dynstr / .dynamic，补段） | 文件增长 |
| 同值重复设置 | no-op | 幂等 |
| `$ORIGIN` 字面量 | 原样写入 | 原样写入 |
| `readelf -d` 可见结果 | 单个 DT_RPATH=值，无 DT_RUNPATH | 同左 |

两者输出不要求逐字节一致：oxipatch 在 .dynstr 整体迁移时会把废弃的旧
rpath 槽清零，patchelf 保留残字节；加载器语义相同，字节差异不作为验收项。

## 5. 为什么同时兼容两个方案

oxipatch 是 2026 年 10 月首发的 0.1.0 新库（单作者、CI 只覆盖 x86_64），
patchelf 是多年生产验证的工具。保留双后端 = 风险对冲：默认用 oxipatch
（零外部依赖、免进程启动），发现任何边界行为问题时，用户改一行
`rpath_backend = "patchelf"` 即可回退，不需要改代码或重编 phprt。
两个后端靠第 9 节的差分测试保证语义一致。

## 6. patchelf 后端的前置检查

选 `patchelf` 时不再把它放进产品 profile 的 `tools` 清单；改为
`build::run()` 开始时（与 tools 检查同属「下载源码之前」的失败快）
探测一次 `patchelf` 命令，缺失即报：

```text
error: rpath_backend = "patchelf" 需要系统安装 patchelf（Fedora: sudo dnf install patchelf）
```

## 7. ARM 兼容

phprtv3 当前只有 x86_64 产物，但后端选择不得把 ARM 排除在外：
oxipatch 的数据模型按 class / endian 泛化（显式支持 32/64 位、LE/BE，
测试套件含 elf32 大端 fixture），rpath 操作本身与 e_machine 无关；
patchelf 命令后端天然随系统工具走。两条后端在 ARM 上都需实测验证（见第 9 节）。

## 8. 实施时的改动清单

| 文件 | 改动 |
| --- | --- |
| `Cargo.toml`（workspace） | 加 `oxipatch`、`phprt-rpath` 依赖；members 加 `crates/rpath` |
| `crates/rpath/` | 新 crate：Backend / Options / set_rpath，两个后端实现 + 单元测试 |
| `src/config.rs` | `rpath_backend` 字段 + 默认值 + 校验 + 转 `phprt_rpath::Backend` |
| `src/build.rs` | `Options` 加 `rpath` 字段；删 `patchelf_rpath()`；`deploy_libs` 改调 crate；`run()` 开头按需探测 patchelf |
| `src/main.rs` | 构造 `build::Options` 时传 backend |
| `recipes/products/php/_profile.star`、`recipes/products/nginx/_profile.star` | 删 `tool("patchelf", min = "0.13")` |
| `recipes/phprt.toml` | 加第 3 节的注释键 |
| 文档 | BUILD.md §6 第 2 条与 §7 的 dnf 行；README 工具表与报错示例；CLI.md:317；PRODUCT.md 的 tools 示例与固定名单表述；CONTRACT.md:449「patchelf 输入」措辞；DESIGN.md crates 列表与 config.rs 一行 |

## 9. 验收

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets
```

差分测试（两条后端都跑，语料逐个比对）：

- 语料：PHP 主二进制、扩展 `.so`、nginx、二进制与库各一份「已有 RUNPATH」
  （CMake BUILD_RPATH 产物）、一份「已有 RPATH」、一份「无 rpath」、
  一份真实 aarch64 / armv7 ELF。
- 比对项：`readelf -d` 的 DT_RPATH 值与无 DT_RUNPATH 残留；`ldd` 解析正常；
  可执行物能跑（`php -v`、`nginx -V`）；oxipatch 后端前后文件大小变化合理。

命令级验收（AGENTS.md 第 5 节全量矩阵之外，加跑两遍）：

```bash
# 默认 oxipatch
cargo run -- install php@8.5
./.phprt/php-8.5.12/bin/php -v
readelf -d .phprt/php-8.5.12/bin/php | grep -E 'RPATH|RUNPATH'

# 切到 patchelf 后端重装同版本（--force），结果与上一致
# phprt.toml: rpath_backend = "patchelf"
cargo run -- install php@8.5 --force

# patchelf 后端但命令未安装 → 报第 6 节的 dnf 提示
```

## 10. 风险与回退

- oxipatch 太新：默认后端就是它，靠差分测试兜底；出问题改配置一行回退，
  不需要发版。
- ARM 未经上游 CI 验证：差分语料必须含真实 ARM ELF，通过前不算改完。
- 构建签名不受影响：`runtime` 节点的输入（`lib_dir` / `rpath` 值）没变，
  只是执行工具换了；CONTRACT.md 相关措辞按第 8 节同步。
