# 解压

`install` / `update` 在下载阶段之后，把方案里的源码包按构建顺序解压到
`build/<组件>/<补丁>/`。解压后的目录就是源码根，后面的 `phpize` / `./configure`
都在这里执行。

## 1. 触发方式

```bash
phprt install 8.5                    # 下载 → 解压 → 编译 → 写 state
phprt install 8.5 --ext swoole       # 扩展和依赖一起下载、解压
phprt install 8.5 --downloads        # 只下载，不解压
phprt update --offline               # 用缓存下载，再解压
```

- 解压清单和下载清单是同一份：`php`、`dep`、非内置 ext（origin 推导为 pecl） 的 `ext`。
- builtin 扩展的源码在 PHP tarball 里，不单独解压。
- `--downloads` 在下载结束后就停止，不进入解压。
- 解压失败时命令退出 1，不写 state；已经解压完的目录保留。
- 解压完成后进入 [BUILD.md](BUILD.md) 的编译阶段。

## 2. 顺序

按 `plan` 打印的拓扑序遍历，串行解压：

```text
依赖库 → 静态扩展 → PHP → 共享扩展
```

实际顺序由依赖边决定，不额外排序；解压本身不依赖前一个目录，但日志顺序和构建顺序一致。

## 3. 目录布局

```text
<root>/
├── .phprt/downloads/php-8.5.12.tar.xz      # 压缩包缓存
└── build/
    ├── php/php-8.5.12/                     # unpack
    ├── libcurl/curl-8.23.0/
    ├── nghttp2/nghttp2-1.64.0/
    └── swoole/swoole-6.2.5/
```

- 目录名 = 组件名 + 压缩包去后缀的 stem；补丁升级会落到新目录，旧目录不自动删除。
- `build/` 在项目根，不在 `.phprt/` 下；删除它只会触发重新解压，不影响下载缓存。
- `unpack` 路径由 `src/layout.rs` 统一计算，计划输出、下载清单、解压共用。

## 4. 格式

| 后缀 | 解码 | crate |
| --- | --- | --- |
| `.tar.gz` / `.tgz` | gzip + tar | `flate2` + `tar` |
| `.tar.xz` / `.txz` | xz + tar | `lzma-rust2` + `tar` |
| `.tar.bz2` | bzip2 + tar | `bzip2` + `tar` |
| `.zip` | zip | `zip` |

都是纯 Rust 解码，不依赖系统的 `tar` / `xz` / `unzip`。未知后缀直接报错并列出支持列表。

## 5. 原子解压与跳过

1. 目标目录已有内容 → 跳过，输出 `✓ 已解压`。
2. 目标目录存在但为空 → 删掉重解。
3. 解压先写到同级临时目录 `.<stem>.tmp-<pid>`。
4. 压缩包顶层只有一个目录时，把它提上来作为目标目录。
5. PECL 包是 `package.xml` + 单个源码目录，只把源码目录提上来，`package.xml` 丢弃；
   其他多条目布局按原样使用。
6. 全部成功后再改名为目标目录；失败只删临时目录，不会留下半份源码。
7. 开工前清理同一目标遗留的 `.<stem>.tmp-*`。

这样中断或失败都不会让“目录存在但源码不完整”被误判为已完成。

## 6. 安全

- tar：逐条 `Entry::unpack_in`；包含 `..` 的条目直接失败，父目录越界也会被拒绝。
- zip：只用 `enclosed_name()`，拒绝绝对路径和 `..`；zip 内的符号链接直接报错。
- 保留 tar 里的权限位（`configure` 等脚本仍可执行）。
- 压缩包本身不重复校验；下载阶段已经做过 sha256 / 大小校验。

## 7. 输出

终端里和下载共用同一套进度框；逐行模式下：

```text
解压源码（按构建顺序，3 个）
[1/3] [php] 8.5.12 解压 php-8.5.12.tar.xz → build/php/php-8.5.12
[php] 8.5.12 ✓ 解压完成
[2/3] [deps] nghttp2 1.64.0 解压 nghttp2-1.64.0.tar.gz → build/nghttp2/nghttp2-1.64.0
[deps] nghttp2 1.64.0 ✓ 解压完成
[deps] libcurl 8.23.0 ✓ 已解压
解压完成: 2 个，已存在 1 个
```

- tar 系列的进度总量是压缩包字节数；zip 是各文件解压后大小之和。
- 全部命中缓存时每个源码包输出一行 `✓ 已解压`，再加汇总。

## 8. 失败与退出码

| 情况 | 行为 |
| --- | --- |
| 源码包不在缓存 | 报错并提示先运行 `--downloads`（正常流程不会出现） |
| 不认识的压缩格式 | 退出 1 |
| 压缩包损坏 / 越界路径 | 退出 1，目标目录不产生 |
| 目标已存在且不是目录 | 退出 1 |
| Ctrl-C | 130，临时目录由下次运行清理 |

解压失败不写 state；已经完成的目录保留，下次直接从失败的那个继续。

## 9. 代码位置

| 文件 | 职责 |
| --- | --- |
| `crates/extract/src/lib.rs` | 顺序调度、跳过判断、临时目录、失败清理 |
| `crates/extract/src/archive.rs` | 后缀分派、tar/zip 解包、解压进度 |
| `crates/source/src/lib.rs` | 下载/解压共用的 `Source` 清单 |
| `crates/ui/src/lib.rs` | 进度框、逐行日志、标签与大小格式 |
| `src/layout.rs` | `build/<组件>/<stem>` 路径计算 |

## 10. 测试

新增 crate 内测试覆盖：

- `.tar.gz` / `.tar.xz` / `.tar.bz2` / `.zip` 各解一次；
- 单顶层目录被提平，扁平包保持原样；
- 可执行位保留；
- 已存在目录跳过，空目录重解；
- 未知后缀、缺压缩包、损坏压缩包报错，且不留下目标目录或临时目录；
- tar 越界条目（`../evil.txt`）被拒绝，越界文件没有写到外面。
