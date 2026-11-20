# catalog 自动更新

状态：已实现 openssl / zlib / php / 13 个 PECL 扩展 / 5 个目录索引源 / webp / aom / 9 个 GitHub Releases 组件 / 6 个 GitHub tag 归档组件共 38 源并过验收（2026-11-20，第 14 节有实测结果）；其余源按 12.4 分批。

目标读者：维护 `recipes/catalog.toml` 的人，以及实现这个功能的 agent。

## 1. 目的

上游发布新补丁后，把「最新版本的下载地址和 sha256」写进 `recipes/catalog.toml`，替代手工查版本、查跳转、下载算哈希。

约束：

- 只改 `catalog.toml`，不改 recipe、bundle、state。
- 查询以组件为单位：同一组件的多条线共享一次请求的结果，不按线重复访问同一地址。
- 写文件前先备份现有 catalog。
- 查不到、算不出哈希的条目宁可不写，也不写半条。
- 先验证 openssl 和 zlib 两个源，行为确认后再扩大覆盖面。

## 2. 现状

### 2.1 v2 的 `outdated`

v2 的查询能力在 `phprtv2/runner/upstream.sh`，`outdated` 只读：

| 能力 | 位置 | 说明 |
| --- | --- | --- |
| `upstream=github:owner/repo` | recipe 头部 | GitHub tags，取版本号 |
| `upstream=pecl:包名` | recipe 头部 | PECL stable 列表 |
| `upstream=phpnet:系列` | recipe 头部 | php.net 某系列最新版 |
| `versions()` | recipe 内 shell 函数 | 特殊来源，用 `upstream_versions_listing` 等公共函数 |

行为要点：

- 只输出「已装 / 安全更新 / 最新」，不改任何文件。
- 预发布过滤：名字含 alpha/beta/rc/dev，或最后一段数字 `>= 90`。
- 安全更新按「前两段不变」判断。
- 结果缓存 6 小时；`OFFLINE=1` 不联网。
- URL 由 recipe 的模板替换版本号得到，不检查跳转。

v2 没有「写回 catalog」这一步，也没有 sha256 处理。这部分是新需求，不能照搬。

v2 把查询规则写在 recipe 里（`upstream=` / `versions()`），v3 不沿用，载体见 4.1。

### 2.2 v3 的现状

| 事实 | 影响 |
| --- | --- |
| `catalog.toml` 只有 `version` / `url` / `sha256` | 没有任何「去哪里查上游」的信息 |
| recipe 是 Starlark，加载时要用 catalog 注入版本 | 查询规则不能写在 recipe 里，否则工具要先有一份正确的 catalog |
| catalog 只放结果（`version` / `url` / `sha256`） | 查询规则也不写进 catalog；规则需要单独的载体，见第 4 节 |
| `phprt update` 只跟随**已有** catalog | catalog 不更新，`update` 升不上去 |
| `Catalog::load` 会校验缺 `url`、重复版本 | 写坏文件会直接让所有命令失败 |
| catalog 里有大量注释和分组 | 不能整体反序列化再写回，否则注释丢失 |

## 3. 范围

做：

- 查询上游某个组件在**已有版本线**内的最新稳定补丁；
- 生成下载地址，下载文件本身计算 sha256；
- 备份后写回 `catalog.toml`；
- 回填 catalog 里已有条目的 sha256；
- 文本和退出码报告哪些组件更新、跳过、失败。

不做：

- 新增版本线（openssl 4.0 → 4.1、PHP 8.5 → 8.6 等）。新线由开发人员按 [ADD-RECIPE.md](ADD-RECIPE.md) 主动添加，工具不写、不改。
- 修改 `bundles/`、recipe 的 `LINES` / 兼容性约束。
- 自动执行 `phprt update` 或 `install`。
- 验证 catalog 更新后 `add` / `update` 的行为；这不在本次范围。
- 独立 lock 文件、后台定时任务。

## 4. 数据结构

### 4.1 载体：每组件一个 star 脚本

查询规则不写 recipe（recipe 加载时要用 catalog 注入版本，规则写进去会循环依赖），
也不写 catalog（catalog 只回答有哪些补丁、从哪下载）。v2 把规则写在 recipe 头部
（`upstream=` / `versions()`），v3 不沿用。

每个组件一个「类似 recipe 的契约模板化 star 脚本」，只约定三件事：

1. 提取的 URL 地址：访问哪个地址能拿到版本和下载连接列表；
2. 下载连接提取：访问后如何从返回内容里解析出下载连接；
3. 版本提取：如何从 tag / 文件名里取出版本号，例如 `v2.1.33` 过滤掉 `v`。

线归属、稳定性过滤、版本比较不在脚本里，在 Rust 侧（第 6 节）。
脚本不关心新版本线：只有 catalog 声明过的主线版本才触发更新（见 5、6.2）。

文件放在 `recipes/upstream/<组件名>.star`，组件名与 catalog 的 key 一致；
提供模板，新组件照抄改三处。脚本 API 已定：顶层一个 `UPSTREAM` 字典，

| 槽位 | 必需 | 内容 |
| --- | --- | --- |
| `url` | 是 | 访问哪个地址拿版本和下载连接列表；可含 `{line}`，只有 URL 本身按线划分的源才写（如 php.net） |
| `link` | 是 | 从返回内容抽下载连接的正则，必须有命名组 `link` |
| `version` | 是 | 从下载连接抽版本的正则，必须有命名组 `version`（`v2.1.33` 这类前缀在正则里过滤掉） |
| `join` | 否 | 连接是相对地址时拼绝对地址的模板，如 `"https://x/fossils/{link}"`；源按线分目录时可含 `{line}`（此时 `url` 必须也带 `{line}`） |

槽位缺名、类型不对、正则不合法都在加载时报错，不静默忽略。示例见
`recipes/upstream/openssl.star`（GitHub Releases API）、`zlib.star`（目录索引 + join）
和 `libxml2.star`（GNOME 按线分目录，url 和 join 都带 `{line}`）。

### 4.2 上游源的形态（事实记录）

不再有 kind 枚举；每个组件的脚本自己处理自己的源形态。已实测的形态：

| 源形态 | 代表组件 | 版本在哪 | 下载连接在哪 | 哈希情况 |
| --- | --- | --- | --- | --- |
| php.net JSON | php | `releases/index.php?json&version=<线>` 返回 | 接口的 `filename` | 接口自带 sha256 |
| PECL XML | swoole / xdebug 等 | `rest/r/<包>/allreleases.xml` | 模板 `https://pecl.php.net/get/<包>-<版本>.tgz` | 无 |
| GitHub Releases | openssl | Release 的 `tag_name` | asset 的 `browser_download_url` | asset 带 `digest` |
| GitHub tag | avif、libmemcached 等 | Tags API | tag 归档模板 | 无 |
| 目录索引 | zlib（fossils）、GNOME、Google Storage | 页面 / S3 XML 里的文件名 | 页面里的 href | GNOME 有 `<名字>.<版本>.sha256sum`，其余无 |

php.net 的 URL 带线，属于源形态决定的按线访问；其余形态一次请求即可覆盖该组件的全部线
（见 5 的执行原则）。

## 5. 流程

```text
读取并校验 catalog.toml
  → 对每个组件（点名或全部）：加载 recipes/upstream/<组件>.star
      没有脚本：回填模式记「跳过」，点名更新记「失败」
      → URL 不含 {line}：执行一次脚本查询，得全部 (版本, 下载连接) 候选
  → 对每条 catalog 已有线：
      → 过滤预发布 / 不稳定，只留属于该线的候选，取最大版本
      → 不比当前最大补丁大：记「无更新」
      → 核对文件名版本与提取版本一致
      → Range 0-0 探测连接可达（跟重定向，复用镜像候选）
      → --dry-run 到此为止，只打印计划
      → 全部线判断完，把要下载的条目一次交给下载队列算 sha256（10 个文件 / 每站点 2 个）
      → 按原顺序追加到线数组末尾
  → --fill-sha256：已有条目里 sha256 为空的进入同一个下载批次补齐
  → 有改动且非 dry-run：备份 catalog → 写临时文件 → Catalog::load 复检 → 原子替换
  → 打印报告（新增 / 回填 / 无更新 / 跳过 / 失败）；有组件失败退出码 3
```

不处理新版本线：

- 候选版本不属于任何已有线时，不写入；
- 需要新增线时由开发人员按 [ADD-RECIPE.md](ADD-RECIPE.md) 第 2 节判断并手工添加。

执行原则：查询以组件为单位做一次。脚本一次返回该组件的全部候选，Rust 侧再按
catalog 已有线做归属过滤；同一组件的多条线（如 openssl 的 `1.1.1` / `3.0` / `3.5` /
`4.0`）共享同一次请求的结果，不按线重复请求同一个地址。这是设计要求，不是可选项：
人类开发会默认这么做，agent 实现时容易退回「按线逐个请求」，写进流程就是为了防止这种退化。
只有源形态本身强制按线访问时（如 php.net 的 URL 带 `<线>`、GNOME 按线分目录）才按线请求，并在脚本里注明原因。
顺带效果：GitHub 匿名限流按请求计数（见 11），同组件只请求一次就不吃重复配额。

## 6. 版本与线的判定

### 6.1 稳定性过滤

沿用 v2 的判据：

- 含 `alpha` / `beta` / `rc` / `dev` / `pre`：不取；
- 最后一段数字 `>= 90`：视为预发布（libjpeg-turbo 的 `3.1.90` 表示 3.2.0 beta）；
- 只接受点分数字（末段可带字母后缀，如 `1.1.1w`）和点分数字 + 连字符数字后缀
  （如 `7.1.2-35`、`2.1-20261102`）；这两类由第 6.3 节的比较器处理，脚本的提取
  正则负责把 `v` 前缀等不规则形式过滤掉。
- 脚本的版本正则必须保留预发布后缀（`3.2.17RC1` 不能截成 `3.2.17`）：PECL 会把
  RC 标成 `stable`（igbinary 实测），只剩 Rust 侧这层过滤，截掉后缀会让 RC 混进
  catalog。

### 6.2 线归属

对 catalog 里每个已有线 `line`，候选版本 `v` 属于该线，当且仅当：

```text
v == line
或 v 以 "line." 开头
或 v 以 line 开头且下一个字符是 ASCII 字母（处理 openssl 1.1.1w）
```

线本身不改名、不新增。该线没有新补丁时输出「无更新」。

### 6.3 版本比较

两个目标共用同一套比较逻辑（Rust 侧）：

1. 构建时：`install` / `add` / `remove` / `update` 只指定主线版本时，默认取线内最大补丁。
   catalog 在一条线的数组里用同一套 `compare` 取最大；
2. 更新 catalog 时：从脚本提取的候选里判断哪个下载连接是本次要写入的版本。

本次讨论定下的方向：在 Rust 里写多个排序方法——semver 排序、`2.1-20261102` 这类日期后缀
排序、以及其它不规则形式——再用正则区分一个版本走哪个方法；分类不做全局预先设计，
编写每个源的脚本时一步一步加上去（13.1 第 11 条）。

现有 `compare` 的行为是两条路径都在依赖的底线，`3.5.10 > 3.5.9`、`8.5.13 > 8.5.9`、
`1.1.1w > 1.1.1u`、`4.0 == 4.0.0` 不能被新比较器打破。

实现：比较器在 `crates/upstream/src/version.rs`，`classify` 用正则把版本分成两类——
点分数字（末段可带字母后缀，`3.5.9` / `1.1.1w`）和连字符数字后缀（`7.1.2-35` /
`2.1-20261102`，后缀按数值比）；同类走对应比较器，异类退回字符串比较（同一线内
形式一致，不会发生）。`src/version.rs::compare` 已删除，`src/catalog.rs` 取最大补丁
改调 `phprt_upstream::compare`，两个目标真正共用一套。

## 7. 下载地址

下载连接由脚本的第 2 部分（下载连接提取）给出，两种来源：

1. 源直接给链接：GitHub Release asset 的 `browser_download_url`、目录索引页里的 href、php.net 接口的 `filename`；
2. 脚本按模板拼：例如 PECL `https://pecl.php.net/get/<包>-<版本>.tgz`。

写进 catalog 前要满足：

- 地址里的文件名版本号必须和 `version` 一致；
- 写进 catalog 的是脚本提取的**稳定地址**：GitHub Release asset 的最终地址是带签名的临时 URL，会过期，不落盘；下载期由下载器自己跟重定向；
- 可达性用 `Range: bytes=0-0` 探测（接受 200 / 206 / 416），复用下载器的镜像候选和代理逻辑，不另写一套；
- 检查失败就不写这一条，报告失败原因。

## 8. sha256

### 8.1 统一按文件计算

所有组件都走同一条路径：下载 `url` 指向的完整文件，计算 SHA-256，再写进 catalog。

不使用上游提供的 `digest` 字段或单独的 `.sha256sum` / `.sha256` 文件，原因是各源形式不统一：有的接口带哈希，有的只有签名，有的什么都没有。统一下载文件本身，只有一条代码路径，也不引入额外 URL。

上游公开的哈希值只用于测试期交叉校验，例如 php.net JSON 里 8.5.12 的 `08bc2eba…`、openssl 3.5.9 的 `603f5602…`、zlib 1.3.2 的 `bb329a0a…`。不参与写入逻辑。

这是「首次信任」（TOFU）：哈希来自本次下载，能防止之后文件被换，不能证明上游当时没被篡改。文档和输出都不能暗示它等价于官方签名验证。

### 8.2 写入与回填

- 新增条目：下载成功且 sha256 非空才写；下载失败就跳过这条并报告。
- 已有条目：默认不动。
- 回填：`--fill-sha256` 遍历 catalog 里所有 `sha256` 为空的条目，逐个下载补齐；已经是 64 位十六进制的跳过。`--dry-run` 只列出待回填条目，不下载。
- 回填和新增条目共用同一份下载缓存。

### 8.3 缓存文件名

用 `phprt_download::archive_name(url)` 得到缓存文件名，和下载器一致；同一 URL 不会出现两份。回填结束后文件保留在缓存里，下次 `install` 直接命中。

## 9. 写回 catalog

用 `toml_edit` 读改写，保留注释、分组、空行和数组里其它元素的格式；不能 `toml::from_str` 后整体序列化。

写回规则：

- 在对应线的数组末尾插入一条 `{ version = "...", url = "...", sha256 = "..." }`；
- 数组顺序不参与选择（`Catalog::get` 取最大版本），所以不重排、不动旧元素；
- 同一版本已存在则跳过，不产生重复；
- 写临时文件，再用 `Catalog::load` 校验；校验通过才 rename 覆盖；
- 校验失败时保留原文件，提示备份路径。

## 10. 备份与失败处理

- 每次真正写文件前备份一次，不是每个组件备一次；
- 默认位置 `<工具目录>/backup/<UTC 时间戳>_catalog.toml`，和 ini 备份共用 `local_backup_dir` / `global_backup_dir` 配置；
- 备份失败则整个命令失败，不写 catalog；
- 单个组件查询或下载失败不中断其它组件；
- 退出码：全部成功且无失败为 `0`；写入或备份失败为 `1`；有组件失败但其余成功为 `3`（和 v2 `outdated` 的「结果不完整」一致）。

## 11. crate 与代码落点

新增 crate `crates/upstream`：

| 文件 | 职责 |
| --- | --- |
| `script.rs` | 加载并执行组件的 upstream star 脚本，拿到 URL / 下载连接 / 版本提取结果 |
| `source.rs` | 查询适配器，输出候选 `(版本, 下载连接)` |
| `version.rs` | 预发布过滤、线归属、多排序方法比较与正则分类 |
| `artifact.rs` | 文件名版本核对、下载并算 sha256 |
| `catalog.rs` | `toml_edit` 读改写、备份、插入补丁、回填哈希、原子写入 |
| `report.rs` | 结果结构、文本输出、`partial_failure` 标记（退出码由 main 判） |
| `lib.rs` | 对外入口：`run(Options) -> Report` |

`src/main.rs` 新增子命令，只做参数解析和调用，不放查询逻辑。

解耦约束（实现时不允许退化）：

- Rust 侧不出现按组件名分发的分支；组件差异全部在 star 脚本里，Rust 只消费统一的
  `(版本, 下载连接)` 候选列表。
- 线归属、稳定性过滤、版本比较是对候选列表的通用函数，构建时取最大和更新时认目标
  共用同一套，不各写一套。
- 下载、镜像候选、代理、哈希计算复用 `phprt-download` 的现有路径，不复制第二份。
- 比较器用「正则 → 比较器」的表驱动分发：新增不规则形式是加表项，不是加代码路径；表项随源逐个添加。

需要说明的取舍：

- 比较逻辑是两个目标共用的（第 6.3 节）。已落在 `crates/upstream/src/version.rs`（带单测）；`src/version.rs::compare` 已删除，`src/catalog.rs` 改调 `phprt_upstream::compare`。
- star 脚本求值复用现有 starlark 依赖：`crates/upstream` 直接依赖 `starlark`，脚本解析在 `script.rs::Script::parse`；recipe 的 `eval_meta` / `eval_target` 仍在二进制里，两处不共享代码。
- 下载复用 `phprt-download`：`final_url` / `run_outcomes` / `fetch_text` / `archive_name` 已导出为 `pub`，catalog 更新和 install 走同一条下载路径，不复制第二份；需要下载的条目先全部收进一个批次，一次交给下载队列（10 个文件 / 每站点 2 个），不逐条串行。
- GitHub 匿名 API 每小时 60 次。一个仓库一次请求，约 15 个 GitHub 组件，够用；`fetch_text` 对 github.com URL 自动带 `GITHUB_TOKEN` Bearer，限流失败按组件报告。

## 12. 测试计划

### 12.1 阶段一：无网络单测

`crates/upstream` 内：

| 测试 | 断言 |
| --- | --- |
| 预发布过滤 | `6.2.6RC1`、`3.1.90`、`2.0.0-beta1` 被丢弃 |
| 线归属 | `8.5.13` 属于 `8.5`；`1.1.1w` 属于 `1.1.1`；`4.1.0` 不属于 `4.0`，不写入 |
| 脚本三件套 | fixture 脚本的 URL / 下载连接 / 版本提取各跑通一次，含 `v2.1.33` → `2.1.33` 的过滤 |
| 版本比较 | `3.5.10 > 3.5.9`、`8.5.13 > 8.5.9`、`1.1.1w > 1.1.1u`、`4.0 == 4.0.0`；日期后缀类 `2.1-20261102 > 2.1-20261101`；正则把两类分到不同比较器 |
| 插入格式 | 在带注释的 catalog 片段里插入后，注释、缩进、其它元素逐字不变；再次解析能通过 `Catalog::load` |
| 重复版本 | 已有版本不重复插入 |
| 缺哈希 | 下载失败 / 哈希为空的候选不写入 |
| 备份 | 写入前生成一份内容相同的备份；备份失败时不写 |

### 12.2 阶段二：只测 openssl 和 zlib

在临时目录复制 `recipes/`，人为把两条线改成旧补丁，再运行工具把新版本抓回来。

| 源形态 | 夹具（人为改旧） | 期望 |
| --- | --- | --- |
| GitHub Releases | `openssl 3.5` 线只保留 `3.5.8` | 补上新补丁（`>= 3.5.9`）；URL 来自 Release asset；下载文件算出的 sha256 与写入值一致；文件名版本与 `version` 一致 |
| 目录索引 | `zlib 1.3` 线只保留 `1.3.1` | 补回 `1.3.2`；URL 来自 fossils 页面的 href；sha256 等于 `bb329a0a…` |

openssl 的上游可能已经超过 3.5.9，所以不强制等于 `603f5602…`；只要版本大于 3.5.8、哈希能被同一文件复算即可。zlib 的 1.3.2 是已知的线内最新版本，要求逐字等于原 catalog 的值。

两项都要跑两遍：第二遍必须是「无更新」，catalog 文件字节不变。

这两个源覆盖两类脚本提取路径（Release asset 直链、目录索引 href 解析）和同一条哈希路径，先跑它们能在扩大范围前暴露脚本执行、解析、下载、写回四类问题。

### 12.3 阶段三：sha256 回填

在临时 catalog 里清空一个小条目的 sha256（例如 `xdebug 3.5.3`），运行：

```bash
cargo run -- catalog update --fill-sha256 xdebug
```

期望：

- 补回的值等于原 catalog 的 `f073de91…`；
- 再跑一遍不改文件；
- 已填的条目不重新下载。

### 12.4 阶段四：其余源

按来源分批补组件的 star 脚本，每批只加一类：

| 批次 | 组件 | 新问题 |
| --- | --- | --- |
| 4.1 | php、其余 PECL 扩展 | 接口格式、批量下载耗时 |
| 4.2 | GNOME / savannah / GNU FTP / libsodium / pyyaml | 目录索引格式差异 |
| 4.3 | Google Storage（webp / aom） | S3 XML 列表解析；webp 的 GCS 目录无列表页，改走 tags API + join |
| 4.4 | 其余 GitHub Releases（pcre2、c-ares、libssh2、nghttp2、patchelf 等） | asset 命名差异 |
| 4.5 | GitHub tag 归档（avif、libmemcached、rabbitmq-c、librdkafka 等） | tag 无直链，必须模板拼 |
| 4.6 | curl、sqlite、libpng | 页面特殊、文件名与版本不直接对应 |
| 4.7 | luajit、lua-resty-*、ImageMagick | tag 形式不规则；接每个源时同步加上它的比较器分类 |

每批完成后跑同一套夹具：把已知最新补丁清掉 → 抓回 → 比对；再跑一遍确认幂等。全部批次完成后再做一次全量回填和全量「无更新」检查。

进度：

- 4.1 已完成（2026-11-20）：php（php.net JSON，按线访问是接口形态强制的）+
  swoole / xdebug / redis / apcu / igbinary / msgpack / protobuf / yaml /
  memcached / imagick / ssh2 / amqp / rdkafka 共 13 个 PECL 扩展。
- 4.2 已完成（2026-11-20）：libxml2（GNOME，按线分目录）/ freetype（savannah）/
  autoconf（GNU FTP）/ libyaml（pyyaml.org）/ libsodium，全部是目录索引页 + join。
- 4.3 已完成（2026-11-20）：aom（GCS S3 XML 列表）/ webp（GCS 目录是 2014 年的空
  对象、没有列表页，改走 GitHub tags API + GCS join 模板，与 PECL 同构）。
- 4.4 已完成（2026-11-20）：pcre2 / c-ares / libssh2 / nghttp2 / patchelf / jpeg /
  libheif / libde265 / oniguruma 共 9 个 GitHub Releases 源。asset 前缀各写各的
  （`pcre2-`、`c-ares-`、`libssh2-`、`nghttp2-`、`patchelf-`、`libjpeg-turbo-`、
  `libheif-`、`libde265-`、`onig-`），`patchelf-<版本>-x86_64.tar.gz` 这类平台变体
  靠版本正则排除。下载从逐条串行改成整批进下载队列（10 文件 / 每站点 2）。
- 4.5 已完成（2026-11-20）：avif / libimagequant / libultrahdr / libmemcached /
  rabbitmq-c / librdkafka 共 6 个 GitHub tag 归档源。Tags API 只给 tag 名，下载
  地址按 `archive/refs/tags/{link}.tar.gz` 拼；version 正则带首尾锚点，`rc` /
  `-RC4` / `R1` 这类后缀 tag 不进候选。libimagequant 的 4.x tag 不属于 catalog
  的 2.18 线，靠线归属过滤。

## 13. 决策与讨论记录

### 13.1 已定

1. 命令形态：`phprt catalog update [--dry-run] [--fill-sha256] [组件名...]`。
2. 新版本线不处理，由开发人员主动添加。
3. 本次实现 sha256 回填，用 `--fill-sha256` 显式触发。
4. 写回引入 `toml_edit` 依赖，保留注释。
5. 新增 crate `crates/upstream`。
6. 第 2 阶段只测 openssl 和 zlib，人为改旧后抓回。
7. 查询规则的载体：每组件一个契约模板化 star 脚本，不写 recipe、不写 catalog；脚本只约定三件事——提取的 URL、下载连接提取、版本提取。
8. 版本比较在 Rust 侧写多个排序方法（semver、日期后缀类、其它不规则形式），构建时取最大与更新时认目标共用同一套。
9. 只有 catalog 声明过的主线版本触发更新；脚本不关心新线。同一组件的多条线在一次查询里提取，不按线重复请求。
10. sha256 由工具下载文件后计算，不用上游校验和 / 签名（讨论记录见 13.2）。
11. 比较器分类不做全局预先设计：编写每个源的脚本时，一步一步把该源版本形式对应的分类加上去。
12. catalog 存脚本提取的稳定地址；GitHub Release asset 的最终地址带签名、会过期，只在可达性探测时用，不落盘。
13. `--dry-run` 只做文件名版本核对 + `Range: bytes=0-0` 可达探测，不下载、不写文件、不生成备份。
14. `--fill-sha256` 只补 `sha256` 为空的条目，受组件名过滤；组件没有 upstream 脚本时 fill 模式记「跳过」而非失败。
15. 比较器下沉到 `crates/upstream`，`src/version.rs::compare` 删除，`src/catalog.rs` 改调 `phprt_upstream::compare`。

### 13.2 sha256 的获取方式（已定）

结论：sha256 由工具下载文件后计算，不用上游校验和 / 签名；第 8 节按此实现。
当时两个立场中「统一自己下载文件算」成立，「用官方校验和 / 签名」被否决。
当时的论据保留如下：

| 立场 | 论据 |
| --- | --- |
| 统一自己下载文件算（采纳） | 只有一条代码路径，所有源都能用；catalog 的 `sha256` 是安装期固定期望值，离线、镜像、以后重装都要用；官方校验和覆盖不全（zlib、PECL、GitHub tag 归档、sqlite、Google Storage 等没有），格式也不统一（PGP / minisign）；验签要管密钥、轮换、吊销，是另一套工程 |
| 用官方校验和 / 签名（否决） | 有官方 sha256 字段或文件的源可以省一次下载；PGP 签名能验证发布者身份，这是哈希给不了的；同源取哈希防不了源站本身被控，自己下载算也一样防不了，所以「省下载」是主要收益 |

补充事实：php.net JSON 自带 sha256；GitHub Release asset 带 `digest`；GNOME 有 `.sha256sum`；curl 只有 `.asc`；zlib、PECL 没有任何校验和；libsodium 用 minisign。`sha256` 还进构建签名和完成标记的 `resolution=`，没有它，同版本文件被换掉不会触发重编。

### 13.3 不规则版本的比较（分类已定）

方向已定并落地：Rust 里写多个排序方法——semver 排序、`2.1-20261102` 这类日期后缀
排序、以及其它不规则形式——用正则区分一个版本走哪个方法；分类不做全局预先设计，
编写每个源的脚本时一步一步加上去（13.1 第 11 条）。

实现：比较器在 `crates/upstream/src/version.rs`，`classify` 把版本分成两类——点分
数字（末段可带字母后缀，`3.5.9` / `1.1.1w`）和点分数字 + 连字符数字后缀
（`7.1.2-35` / `2.1-20261102`，后缀按数值比）；同类走对应比较器，异类退回字符串
比较（同一线内形式一致，不会发生）。新增不规则形式是在 `classify` 加表项，不是加
代码路径。`src/version.rs::compare` 已删除，`src/catalog.rs` 取最大补丁改调
`phprt_upstream::compare`，构建时取最大与更新时认目标真正共用同一套。

背景事实（为什么需要单独处理）：

| 组件 | 上游形式 | 问题 |
| --- | --- | --- |
| ImageMagick | tag `7.1.2-35` | `compare` 按 `.` 分段后最后一段是 `2-35`，后缀按字符串比：`7.1.2-9` 与 `7.1.2-10` 会比出 `-9 > -10`，方向反；多 tag 取最大也可能挑错 |
| luajit | tag `v2.1-20261107` | catalog 里版本只写 `2.1`，tag 里的日期没法直接变成补丁号 |
| lua-resty-core / lrucache | tag `v0.1.32` | 归档地址模板与 tag 的对应关系，归 star 脚本的 URL / 下载连接提取处理 |

未定：

- ImageMagick 的连字符后缀归哪个比较器（当第 4 段数字 `7.1.2.35`，还是后缀里的数字串按数值比），接 ImageMagick 那个源时定。

### 13.4 `--fill-sha256` 的回填范围（已定）

只补当前 `sha256` 为空的条目；已经是 64 位十六进制的跳过，不重算。连已有哈希的
条目全部重算（下载量大，但能发现哈希漂移）不做。回填受组件名过滤：命令行点名了
组件就只处理这些组件。

## 14. 验收

第一轮（第 2 阶段）完成时：

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets

# 临时 catalog 夹具上跑，不碰 recipes/catalog.toml
cargo run -- catalog update --dry-run openssl zlib
```

验收标准：

- openssl 和 zlib 都能把人为改旧的线补回新版本；
- 新条目的 `url` 指向的最终地址可访问，文件名版本与 `version` 一致；
- 新条目和回填条目的 `sha256` 等于对同一文件重新计算的结果；
- 第二遍运行输出「无更新」，catalog 文件的字节不变；
- 非 `--dry-run` 运行时先生成备份，写入后 `Catalog::load` 通过；
- 报告按组件列出「新增 / 回填 / 无更新 / 跳过 / 失败」，有失败但其余成功时退出码为 3；
- Rust 侧没有按组件名分发的分支；同一组件的多条线共享同一次脚本执行的结果。

第一轮实测（2026-11-20，临时目录夹具 `/tmp/upstream-fixture`，人为把线改旧）：

- `openssl` 3.5 线补上 3.5.9，sha256 `603f5602…`；`zlib` 1.3 线补上 1.3.2，
  sha256 `bb329a0a…`；都与上游公开值一致；
- `xdebug 3.5.3` 回填 sha256 `f073de91…`，第二遍不重新下载；
- 第二遍运行输出「无更新」，catalog 文件字节不变；
- `--dry-run` 不下载、不写文件、不生成备份；
- 点名不存在的组件退出码 3；全量 sweep（没有 upstream 脚本的组件记「跳过」）不改文件；
- `cargo test --workspace` 189 过 / 1 败（`build::tests::deploy_libs_copies_runtime_deps_but_not_build_tools`
  需要环境预置 patchelf，主 checkout 同样失败，非本次回归）；`cargo clippy --workspace --all-targets` 全绿。

第二轮实测（2026-11-20，4.1 批次，同一夹具 `/tmp/upstream-fixture`）：

- php 8.5 线（夹具改旧到 8.5.11）补上 8.5.13，sha256 `3aeb20a7…` 与 php.net
  接口给的值一致；8.2 / 8.3 / 8.4 线同批补上 8.2.36 / 8.3.37 / 8.4.28；8.6 线
  的 `8.6.0RC3` 不被误判为新版本（预发布过滤）；
- swoole 6.2 线补上 6.2.6；xdebug 3.5 线（夹具改旧到 3.5.1）补回 3.5.3，
  sha256 `f073de91…`；redis / apcu / msgpack / protobuf / memcached / imagick /
  ssh2 / amqp / rdkafka 各线报「无更新」；
- `--fill-sha256 yaml` 把清空的 2.3.0 回填为 `bc840480…`；fill 模式下无脚本组件
  （nginx）记「跳过」但条目照常回填；
- igbinary 3.2.17RC1 被 PECL 标成 stable，靠脚本版本正则保留后缀 + Rust 预发布
  过滤拦下（对应 6.1 的规则，已加回归单测）；
- 全量 sweep `--dry-run`：无下载、无备份、catalog 不变；点名不存在的组件退出码 3。

第三轮实测（2026-11-20，4.2 批次，同一夹具）：

- 五个目录索引源（libxml2 / freetype / autoconf / libyaml / libsodium）都把改旧的线
  补回新补丁，哈希逐字等于 catalog 原值（如 libxml2 2.15.4 `98087fd1…`、
  libsodium 1.0.22 `adbdd8f1…`）；
- libxml2 的 GNOME 索引按线分目录，url 和 join 都带 `{line}`：2.11 线查 `/2.11/`
  目录、报「无更新」，不会被 2.15 目录里的版本污染；
- libsodium 直连下载卡住（HEAD / Range 通、全量 GET 不动），工具如实报失败、退出码 3、
  不写半条；夹具设 `HTTPS_PROXY` 后走代理下载成功（catalog update 与 install 同一条
  env 代理路径）；
- 第二遍 7 条线全部「无更新」，catalog 字节不变。

第四轮实测（2026-11-20，4.3 批次，同一夹具）：

- webp 的 GCS 目录（`.../releases/webp/`）实测是 2014 年的空对象（200 但空体），
  没有列表页，改走 GitHub tags API 拿版本、GCS 模板拼下载地址；tag 里的 rc 后缀
  （`v1.6.0-rc1`）不进 link 正则；
- aom 走 GCS S3 XML 列表（`<Key>libaom-<版本>.tar.gz</Key>`，未分页，`.asc` 不进
  link 正则）；
- 夹具把 webp 1.6 线改旧到 1.5.0、aom 3.12 线改旧到 3.12.0，一轮补回 1.6.0 /
  3.12.1，sha256 与 catalog 原值逐字一致（`e4ab7009…` / `9e977518…`）；
- aom 桶里已有 3.13 / 3.14 / 3.15（最高 3.15.1），3.12 线只补回 3.12.1，高线版本
  没有漏进低线（线归属过滤端到端验证）；
- `--fill-sha256` 回填 1.5.0 / 3.12.0 的哈希，并用独立 curl 下载核验逐字一致；
- 第二遍「无更新」、catalog 字节不变；全量 sweep `--dry-run` 无下载、无备份。

第五轮实测（2026-11-20，4.4 批次，直接在 `recipes/catalog.toml` 上把 9 条线改旧）：

- 夹具：pcre2 10.48 / c-ares 1.34.8 / libssh2 1.11.0 / patchelf 0.19.2 / jpeg 3.1.0 /
  libheif 1.23.6 / libde265 1.1.2 / oniguruma 6.9.9 改旧后一轮全部补回最新补丁；
  nghttp2 线内只有 1.64.0，把条目临时写成 1.63.0 后补回 1.64.0；
- 线归属反例：nghttp2 全仓 136 条 Release（`per_page=100` 只取第一页），全局最高
  1.71.0 没漏进 1.64 线；jpeg 全局最高 3.2.1（beta，`3.1.90` 类）没进 3.1 线，补的
  是 3.1.4.1；
- 哈希：libssh2 / patchelf / jpeg / libheif / oniguruma 补出的值与原 catalog 逐字一致；
  pcre2 `929f0b20…`、c-ares `9364b70a…`、nghttp2 `20e73f3c…`、libde265 `554228bd…`
  原来没写 sha256，`--fill-sha256` 回填后用缓存文件独立 `sha256sum` 核验一致；
- 第二遍 9 个组件全部「无更新」，catalog 字节不变，无新备份；
- 清空下载缓存重跑：5 个补丁同时进队列（「并发: 10 个文件（每站点 2）」），全部
  下载成功，sha256 与 catalog 原值一致；
- 全量 sweep `--dry-run`（49 个组件、32 个有脚本、17 个跳过）用 `GITHUB_TOKEN` 跑完：
  40 条线「无更新」、17 个组件「跳过」、0 失败；8 条线发现上游新补丁（php 8.2.36 /
  8.3.37 / 8.4.28 / 8.5.13、openssl 3.0.22 / 4.0.3、swoole 5.1.8 / 6.2.6），dry-run
  不写文件，catalog 字节不变、零下载、零备份。这些补丁留给专门的一次不带 `--dry-run`
  的运行，不混进本批次提交。

第六轮实测（2026-11-20，4.5 批次，夹具 `/tmp/upstream-fixture-4.5`）：

- 单版本线用 `"线" = []` 清空、双版本线改旧（libultrahdr 2.0.0、rabbitmq-c 0.18.0），
  六个组件一轮全部补回：avif 1.3.0、libimagequant 2.18.0、libultrahdr 2.0.2、
  rabbitmq-c 0.18.1、librdkafka 2.16.0，url 与 catalog 原值逐字一致；
- 哈希：avif `0a545e95…`、libultrahdr `aa8d193b…`、rabbitmq-c `0779bd4f…`、
  librdkafka `e6b61de6…` 与 catalog 原值一致；libimagequant 原来没写 sha256，
  本次补出 `790d2593…`；
- libmemcached 1.1 线补出上游新补丁 1.1.5（sha256 `623b57be…`），说明 tag 归档源
  的补丁号也能升；1.2.x 是 cmake 构建且不属于 1.1 线，没有被写进来；
- 六个文件一次进队列同时下载（10 文件 / 每站点 2 个上限），全部成功；
- 空数组线的插入格式正确（`"1.3" = [ { version = ..., url = ... } ]`），catalog 可被
  `Catalog::load` 复检通过。

## 15. 风险

| 风险 | 处理 |
| --- | --- |
| 上游接口改格式 | 解析失败只影响对应组件，报告失败，不写文件 |
| GitHub 限流 | 支持 `GITHUB_TOKEN`，单仓库一次请求；失败按组件报告 |
| 写坏 catalog | 备份 + 临时文件 + `Catalog::load` 复检 |
| 新补丁和当前 recipe 不兼容 | 工具只管版本数据；构建失败仍由 `phprt update` / `add` 暴露，回到 recipe 修复 |
| 自动更新与「主动适配」原则冲突 | 只在线内升补丁，不新增线，不改兼容性声明 |
| TOFU 哈希被误解为官方签名 | 第 8.1 节明确来源和局限 |
| GitHub Release asset 最终地址带签名、会过期 | catalog 存脚本给的稳定地址；可达性探测跟重定向，下载期由下载器跟重定向，签名 URL 不落盘 |
| 回填需要下载大量旧包 | 用下载缓存；`--fill-sha256` 可只传组件名缩小范围 |
