# catalog 快照、manifest 与签名

状态：未排期（设计稿，2026-11-21）。本文只描述阶段 2 / 阶段 3；阶段 1 的组件分片、
用户侧检查、提示和配置见 [CATALOG-MODEL.md](CATALOG-MODEL.md)。代码未实现前，实际行为以
[CLI.md](CLI.md) 和 [CATALOG-UPDATE.md](CATALOG-UPDATE.md) 为准。

## 1. 目标

阶段 1 已经能按组件分片取数据，但缺少三件事：

- **不可变**：`latest` 目录会被覆盖，回滚和复现没有依据；
- **可审计**：不知道一份数据是谁、什么时候发布的；
- **可校验**：分片自身没有哈希和签名，发布主机被控时可以换一份自洽的数据。

阶段 2 在发布层加快照 id、manifest 和签名；阶段 3 把发布物放到 CDN 和镜像。用户命令的
提示、配置、并发和显示都不变，只换取数后端和本地布局。

做：

- 每次发布一个不可变快照，有 id、manifest、分片哈希和签名；
- 客户端只取闭包分片，校验后写本地快照；
- `--snapshot <id>` 指定快照，`--lock` 按 state 记录的快照复现；
- 维护者 `catalog check` 报告新线候选、无候选、失败；
- 静态目录 + CDN，不做动态接口。

不做：

- 服务端选线、账号、按用户裁剪；
- 可变的最新文件（`latest` 只是指向快照 id 的指针）；
- 推送更新；
- 提前把维护者的单文件源拆碎。

## 2. 快照

一次发布 = 一个快照：

- id 用日期加序号，如 `2026.11.21.1`，可排序、可读；
- 内容不可变：同一个 id 永远指向同一份字节；
- manifest 记录每个组件的分片哈希、线状态、最近检查时间；
- manifest 的 sha256 用来核对内容，快照 id 写进 state 和报告；
- manifest 用 minisign 签名，公钥随发行版固定；
- `latest` 只是一个指针，指向某个快照 id。

## 3. 发布物

```text
catalog/v1/
├── index.json                                   # 快照列表 + latest
└── snapshots/<id>/
    ├── manifest.json                            # 组件 → 分片哈希 / 线状态
    ├── manifest.json.minisig                    # 签名
    └── components/<组件>.toml                   # 与阶段 1 的分片同构
```

分片内容和阶段 1 相同，不再重复；区别是文件路径带快照 id，且被 manifest 记录哈希。

manifest：

```json
{
  "schema": 1,
  "id": "2026.11.21.1",
  "generated_at": "2026-11-21T03:12:00Z",
  "tool_min": "0.3.0",
  "components": {
    "openssl": {
      "shard_sha256": "…",
      "checked_at": "2026-11-21T03:01:00Z",
      "status": "ok",
      "lines": {
        "3.5": { "patches": 5, "status": "ok", "checked_at": "2026-11-21T03:01:00Z" }
      }
    }
  }
}
```

`index.json`：

```json
{
  "schema": 1,
  "latest": "2026.11.21.1",
  "snapshots": [
    {
      "id": "2026.11.21.1",
      "schema": 1,
      "tool_min": "0.3.0",
      "published_at": "2026-11-21T03:20:00Z",
      "manifest_sha256": "…"
    }
  ]
}
```

线状态：

- `ok`：本轮检查成功；
- `stale`：本轮没检查成功，沿用上一份最后已知可用数据；
- `no_candidate`：上游返回里没有属于这条线的版本（老线掉出接口范围时需要人看）；
- `retired`：线已停止维护，分片和旧补丁保留，但不再查询。

组件级 `status` 取所有非退役线里最差的一个（`stale` > `no_candidate` > `ok`）；全部线
都退役时为 `retired`。`stale` 和 `no_candidate` 不阻止发布，但会进 manifest、进 CI 报告、
在客户端 `update` 时提示「某组件数据已 N 天未更新」。

## 4. 维护者命令

```bash
phprt catalog check [组件...] [--json]          # 只查不写：补丁、新线候选、无候选、失败
phprt catalog update [组件...] [--fill-sha256] [--dry-run]
phprt catalog publish --out dist/catalog --sign ~/.config/phprt/catalog.key
```

- `check`：只读审计。除了补丁判断，还报告**不属于任何已有线的新版本**（例如发现
  openssl 4.1、php 8.7），以及无候选和失败。不写 catalog，退出码用于 CI。
- `update`：维护者的写入口，语义不变（只升已有线、sha256 下载后计算、写前校验）。
- `publish`：在阶段 1 的拆分、校验、哈希之外，生成 manifest 和 index，签名，输出到
  快照目录。不访问上游，纯数据变换，可重复执行。

新线候选不自动写入，只进报告；维护者按 [ADD-RECIPE.md](ADD-RECIPE.md) 第 2 节判断后
手工加线。

### 4.1 定时更新

```text
每天 03:00
  → phprt catalog check --json             # 失败 / 新线候选 / stale 报警
  → phprt catalog update --fill-sha256     # 全量，维护者环境，带 GITHUB_TOKEN
  → phprt catalog publish --sign ...       # 生成快照
  → 上传 index + 快照到对象存储 / CDN
  → 若 catalog.toml 有改动，开 PR 供人 review
```

- 发布快照要求**所有组件都有分片**；本轮失败的组件沿用上一份最后已知可用分片，并在
  manifest 标 `stale`。不允许「缺组件」的快照。
- 数据仓库和代码仓库可以分开；用户不需要拿到 `recipes/upstream/*.star`。

## 5. 客户端

用户命令的询问、配置、跳过规则、并发和显示与阶段 1 相同（见
[CATALOG-MODEL.md](CATALOG-MODEL.md) 第 5.0 节），只换取数步骤：

```text
1. GET index.json                     → 选最新且兼容的快照
2. GET snapshots/<id>/manifest.json   → 核对 sha256，验签
3. 求闭包（recipe + bundle + state）  → 得到组件名集合
4. GET components/<名字>.toml         → 只取闭包内的
5. 校验分片 → 写本地快照 → 切换 current
```

- 取数失败、校验失败、签名不对：回退当前快照；一个快照都没有时用随包基线。打印
  「catalog 未更新，仍使用 <id>（<日期>）」，安装继续（交互式 `y`）；`--refresh` 失败
  直接报错。
- 快照里的补丁缺 `sha256`：该组件本次不更新，保留本地数据并报告 `✗`，不挑更旧的补丁
  顶上；提示等待维护者修复。
- 用短超时（秒级）取 index / manifest，不让离线用户等源码下载的长超时。
- 发布地址由发行包在编译时内置，可用 `PHPRT_CATALOG_URL` 覆盖。

本地布局：

```text
.phprt/catalog/
├── current                                  # 当前快照 id
└── snapshots/<id>/
    └── components/<组件>.toml
```

解析时以快照为准，不再叠加随包 `recipes/catalog.toml`；快照缺某个闭包组件时报错并提示
重新同步，不静默补洞。阶段 1 的 `components/` 本地分片不迁移，首次取快照时按快照目录
重建。

`--snapshot <id>` 跳过询问，使用指定快照；本地没有时先取。`--lock` 跳过询问，按 state
记录的 `catalog_snapshot` 和版本解析；快照不在本地时报错，不偷偷换一份。

## 6. 回滚

```bash
phprt install php@8.5 --snapshot 2026.11.01.2   # 用旧快照
phprt install php@8.5 --lock                     # 按 state 记录的版本和快照
```

发布快照不可变，回滚只是切换指针；旧补丁仍在，`--lock` 不受影响。

## 7. 完整性与信任

1. **分片哈希**：manifest 记录每个分片的 `shard_sha256`，下载后先校验再解析。
2. **manifest 哈希**：index 记录 manifest 的 sha256，切换前核对。
3. **manifest 签名**：minisign 公钥随发行版固定；签名不对直接拒绝。

index 不签名也能防篡改内容，但防不住回放：客户端记住见过的最大快照 id，发现 `latest`
变小时告警。

密钥管理：

- 公钥打包进二进制或发行版；
- 轮换时新旧公钥同时信任一个版本周期；
- 私钥只在 CI secret 和维护者离线环境。

**签名不证明上游可信。** 它证明「这份 catalog 是 phprt 维护者发布的」；上游被控时，
签名照样覆盖恶意 tarball 的哈希。所以上游校验和交叉核对仍然要做。

## 8. 在线接口

catalog 是公开、只读、按版本不可变的数据，用静态文件 + CDN：

```text
GET https://catalog.phprt.dev/v1/index.json
GET https://catalog.phprt.dev/v1/snapshots/2026.11.21.1/manifest.json
GET https://catalog.phprt.dev/v1/snapshots/2026.11.21.1/components/openssl.toml
```

- 不鉴权、不区分用户、不记录个人数据；
- 不需要 `GITHUB_TOKEN`；token 只在维护者 CI 查 GitHub 时用；
- 镜像复制同样的路径；客户端按 manifest 校验，镜像无法投毒；
- 对象存储 + CDN 就能承载，没有服务端状态。

缓存头：`index.json` 用 `max-age=300`，快照路径用 `immutable` 和一年缓存。

## 9. 兼容性

- manifest 和 index 有 `schema`；`tool_min` 表示能读这份数据的最低工具版本。
- 客户端在 `index` 里选最新的、自己支持的快照；新 schema 的快照跳过，继续用旧快照。
- 线只追加，不删除。退役线标成 `retired`，旧快照和旧补丁继续可读。
- 发布快照必须包含所有已知组件的分片；客户端需要某个组件而快照里没有时报错并提示
  重新同步，不静默跳过。
- 客户端不认识的字段忽略，不因为多了字段就报错。

## 10. 验收场景

1. **维护者全量**：`catalog check` 报告补丁、新线候选、无候选、失败；`update` 只改
   catalog；`publish` 产出完整快照，所有组件都有分片和 sha256。
2. **客户端取快照**：`install` / `update` 的提示和阶段 1 一样；`y` 后只请求 manifest 和
   闭包分片，网络日志里没有其它产品。
3. **篡改**：改一个分片字节，客户端拒绝激活；改 manifest，验签失败；旧快照仍可用。
4. **回滚**：`--snapshot` 切回旧快照后，`plan` 的版本和当时一致；`--lock` 仍能取到旧补丁。
5. **旧客户端**：遇到新 schema 快照时跳过，选最新的兼容快照，不报「必须升级」。
6. **CI 部分失败**：失败组件标 `stale` 并沿用上一份，快照仍完整；报告和告警能看到是
   哪个组件、多少天没更新。
7. **离线**：`--offline` 下 `install` / `update` / `--lock` 全部可用，不联网。

## 11. 与阶段 1 的关系

| 阶段 1 | 阶段 2 |
| --- | --- |
| `latest/components/<组件>.toml` | `snapshots/<id>/components/<组件>.toml` |
| 分片自带补丁哈希 | 分片哈希 + manifest 哈希 + 签名 |
| 覆盖 `latest` | 不可变快照 + `latest` 指针 |
| 本地 `components/` 覆盖基线 | 本地 `snapshots/<id>/` + `current` |
| 发布主机 + HTTPS | 签发者身份 + 内容哈希 |

用户侧的询问文案、`ask_before_online_update` / `default_answer`、跳过规则、并发显示都
沿用阶段 1；`--snapshot` 只在这里启用。
