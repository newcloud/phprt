# 下载

`install` / `update` 在求解完成、打印计划之后，先把本次方案需要的源码包下载到
`.phprt/downloads/`，再交给解压阶段；`--downloads` 只做下载这一步。
不编译、不创建安装树。

## 1. 触发方式

```bash
phprt install 8.5                 # 下载（命中跳过）→ 解压 → 写 state
phprt install 8.5 --downloads     # 只下载，不解压、不写 state
phprt install 8.5 --ext swoole --downloads
phprt install 7.4 --ext swoole --offline
```

- 方案 = state 里的扩展 ∪ 本次 `--ext` 点名的扩展，和 `install` 完全一致。
- 下载清单直接来自求解结果，不单独再算一遍；缓存命中就跳过。
- 下载失败时命令退出 1，**不写 state**，也不会进入解压。
- `--downloads` 成功时只打印缓存位置，不改 state。
- 解压行为见 [EXTRACT.md](EXTRACT.md)。

## 2. 下载清单

| 节点 | 是否下载 | 缓存文件名 |
| --- | --- | --- |
| `php` | 是 | URL 最后一段，如 `php-8.5.12.tar.xz` |
| `dep` | 是 | 如 `openssl-3.5.9.tar.gz` |
| `ext`（非内置 ext（origin 推导为 pecl）） | 是 | 如 `swoole-6.2.5.tgz` |
| 内置 ext（origin 推导为 builtin） | 否 | 源码在 PHP tarball 里 |

清单按拓扑序生成，顺序只影响开始顺序，实际执行受并发限制。
两个不同的组件若解析出同名文件，直接报错，不静默覆盖。

## 3. 缓存布局

```text
.phprt/downloads/
├── php-8.5.12.tar.xz
├── openssl-3.5.9.tar.gz
├── openssl-3.5.9.tar.gz.part          # 下载中的单流临时文件
├── openssl-3.5.9.tar.gz.part.0        # 分片临时文件
├── openssl-3.5.9.tar.gz.part.json     # 分片元数据
└── swoole-6.2.5.tgz
```

- 缓存跨 PHP 系列共享；删除 `.phprt` 即清空。
- 下载完成后才把 `.part` 改名为最终文件；中途退出不会留下“看起来完整”的包。
- `.part.json` 记录最终 URL、ETag、Last-Modified、总长和分片数。

## 4. 候选 URL

`recipes/phprt.toml` 控制 GitHub 镜像：

```toml
# GitHub 直连失败后是否改用镜像下载：enable / disable
enable_github_proxy_download = "enable"

# 镜像前缀，按顺序尝试，只对 https://github.com/ 生效。
proxy_urls = [
    "https://gh-proxy.com/",
    "https://ghfast.top/",
    "https://ghproxy.net/",
    "https://gh.llkk.cc/",
]
```

```text
原始 URL
→ https://github.com/ 开头的地址追加镜像：
  https://gh-proxy.com/
  https://ghfast.top/
  https://ghproxy.net/
  https://gh.llkk.cc/
```

- 严格按顺序尝试；每个候选最多 2 次（GitHub 直连只试一次），失败才换下一个。
- GitHub 直连单独给 30 秒预算，并且只试一次，超时就换镜像，避免慢连接把整个任务拖住；
  `PHPRT_GITHUB_DIRECT_TIMEOUT` 可以改这个秒数。
- 镜像规则只作用于 `https://github.com/`；php.net、pecl.php.net 不重写。
  openssl.org 的地址会 301 到 GitHub，所以 catalog 直接写 GitHub 地址，见
  [ADD-RECIPE.md](ADD-RECIPE.md)。
- `enable_github_proxy_download = "disable"` 时只请求原始地址，不追加镜像。
- 镜像顺序：`PHPRT_GITHUB_MIRRORS`（空格分隔）非空时用它，空字符串关闭；否则用
  `proxy_urls`；`phprt.toml` 不存在时 `proxy_urls` 用内置 4 个镜像。
- `enable_github_proxy_download` 只接受 `enable` / `disable`，写错直接报错。
- 站点并发按桶计算：GitHub 主站、codeload、raw、objects 和所有镜像共用一个 `github` 桶，
  避免直连失败后所有任务同时压到同一个镜像。

## 5. 并发

| 维度 | 默认 | 参数 |
| --- | --- | --- |
| 同时下载的文件数 | 10 | `--download-jobs` |
| 同一站点同时下载的文件数 | 2 | `--download-host-jobs` |
| 单个文件的分片数 | 4 | `--connections` |

- 文件长度 ≥ 8 MiB 且服务端支持 Range 时才分片；单分片最小 1 MiB。
- 服务端不支持 Range（返回 200）时自动降级为单流。
- 小于 8 MiB 的文件不做分片。

## 6. 断点续传

- 单流：接着 `.part` 的长度发 `Range: bytes=<已有>-`；服务端返回 200 时从头重下。
- 分片：每个分片单独一个 `.part.<n>`，已完成的字节保留；重新运行时只补缺失区间。
- 续传前比对 `.part.json` 里的 URL、ETag、Last-Modified、总长、分片数；
  任意一项变化就丢弃旧临时文件，整包重下。
- Ctrl-C 保留临时文件，光标恢复，退出码 130；下次运行继续。

## 7. 校验与跳过

| 情况 | 行为 |
| --- | --- |
| 目标文件已存在，sha256 匹配 | 跳过，不联网 |
| 目标文件已存在，sha256 不匹配 | 删除后重下 |
| 没有 sha256，能探测到远端大小 | 大小一致就跳过，否则重下 |
| 没有 sha256，探测不到大小 | 按已存在处理并提示 |
| 下载完成，sha256 匹配 | `.part` 改名为最终文件 |
| 下载完成，sha256 不匹配 | 删除并重试一次，仍失败则报错 |
| `--offline` 且缓存里没有 | 报错，不联网 |

目前 catalog 里部分条目没有 sha256，这些只能做大小校验。

## 8. 代理与超时

- 代理：`https_proxy` / `HTTPS_PROXY` / `http_proxy` / `HTTP_PROXY`，遵守 `NO_PROXY`。
- `--proxy URL` 覆盖环境变量，本次所有请求都走它。
- 连接超时 8s，单次读取 60s，单个请求总超时 `--timeout`（默认 900s）。
- 连续 60 秒平均速度低于 1 KB/s 时放弃当前候选，换下一个。
- 重定向手动跟随（最多 6 次），保证 Range 头不会被跳转丢掉。

## 9. 参数

| 参数 | 默认 | 说明 |
| --- | --- | --- |
| `--downloads` | 关 | 只下载，不解压、不写 state |
| `--download-jobs N` | 10 | 文件级并发 |
| `--download-host-jobs N` | 2 | 同一站点并发 |
| `--connections N` | 4 | 单文件分片数 |
| `--timeout N` | 900 | 单个请求总超时（秒） |
| `--proxy URL` | 无 | 覆盖环境里的代理 |
| `--offline` | 关 | 只用缓存，不联网 |
| `--ui auto\|plain\|progress` | auto | 进度显示方式 |

## 10. 进度输出

终端里默认多行进度框，5 Hz 刷新：

```text
源码缓存: .phprt/downloads
并发: 10 个文件（每站点 2），单文件分片 4（≥8 MB 才启用）
  [php] 8.5.12                     [██████████░░░░]  72%  10.1 MB / 13.9 MB  3.4 MB/s
  [deps] openssl 3.5.9             ⠹ 1.2 MB
  [ext] swoole 6.2.5               ⠸ 等待
```

- 未知总长时显示转圈和已下载字节数。
- 完成后固定成一行：`✓ 完成（13.9 MB）`；缓存命中显示 `✓ 已存在`。
- 非 TTY、`TERM=dumb`、设置了 `NO_COLOR` 或 `--ui plain` 时，退回逐行日志。
- `--ui` 同时控制解压阶段；解压按压缩包字节数显示进度。
- `--ui progress` 在非终端里会提示后改用逐行日志。

## 11. 失败输出

```text
下载失败 2 个（文件保留，下次运行可继续）:
  [deps] openssl 3.5.9
    https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz
    原因: 连接超时
    镜像: https://gh-proxy.com/https://github.com/openssl/openssl/releases/download/openssl-3.5.9/openssl-3.5.9.tar.gz
  [ext] swoole 6.2.5
    https://pecl.php.net/get/swoole-6.2.5.tgz
    原因: SHA256 不匹配（期望 ...，实际 ...）
```

镜像提示只对 GitHub 地址出现，方便手工下载。

## 12. 退出码

| 情况 | 退出码 | state |
| --- | --- | --- |
| 全部成功或跳过 | 0 | 写入 |
| `--downloads` 成功 | 0 | 不写 |
| 任意一个失败 | 1 | 不写 |
| Ctrl-C | 130 | 不写，保留 `.part` |
| `--offline` 缺包 | 1 | 不写 |

## 13. 测试

测试用本地 `TcpListener` 起最小 HTTP 服务，覆盖：

- 单流下载与 sha256 校验；
- 大文件分片、拼装、分片请求的 Range 头；
- 分片续传（从已有字节继续）；
- 302 跳转后 Range 仍然生效；
- 500 后重试；
- 服务端忽略 Range 时降级；
- sha256 不匹配报错、删除临时文件；
- 有校验和的文件不联网跳过；
- 无校验和时按远端大小跳过；
- `--offline` 缺包报错；
- 站点并发上限。

真实网络的下载只放在验收里手工跑，不进自动化测试。
