# 状态文件

状态文件只记录**当前构建方案和已编译产物的状态**，不锁定版本。dep 版本仍然按系列
取最新，例如 openssl 3.5.x、php 8.5.x。

## 1. 路径

```text
.phprt/php-<系列>/state.json
```

按系列存放，`8.5.12` → `php-8.5`。补丁版本从 8.5.12 升到 8.5.13 时，方案和
feature 记录不会丢；安装树仍然是 `.phprt/php-8.5.13/`。

## 2. 格式

```json
{
  "version": 1,
  "active": true,
  "php": "8.5.12",
  "ext": {
    "curl": {
      "link": "static",
      "installed": true,
      "sub": { "curl": ["nghttp2"] }
    },
    "swoole": {
      "link": "shared",
      "installed": false,
      "sub": {}
    }
  },
  "deps": {
    "libcurl": { "version": "8.11.1", "features": ["nghttp2", "ssl"] },
    "nghttp2": { "version": "1.64.0", "features": [] }
  }
}
```

| 字段 | 说明 |
| --- | --- |
| `active` | 是否已经写过方案。写过之后 `ext` 就是权威列表，即使为空 |
| `php` | 最近一次求解的 PHP 补丁版本，仅记录 |
| `ext.<名字>.link` | 当前选定的链接方式 |
| `ext.<名字>.installed` | 是否已安装（模拟） |
| `ext.<名字>.sub` | 选中的子配置及其转发 feature |
| `deps.<名字>.version` | 编译时的版本，仅记录，不参与锁定 |
| `deps.<名字>.features` | 当前构建使用的 feature 并集 |

## 3. 读写时机

| 命令 | 读 | 写 | installed |
| --- | --- | --- | --- |
| `plan` / `graph` | 是 | 否 | 不变 |
| `ext list` | 是 | 否 | 不变 |
| `install` / `update` | 是 | 是 | 全部设为 true |
| `ext add` | 是 | 是 | 新扩展 false |
| `ext remove` | 是 | 是 | 删除条目 |

第一次写状态时，用 PHP recipe 的 `exts` 补种方案。写之前，这些默认扩展在
`ext list` 里按 `pending` 显示。

## 4. feature 规则

- `ext add` / `install` / `update`：`deps.features` 取并集，只增不减。
- `ext remove`：按剩余扩展的 sub 重新计算，允许收窄。
- 状态里记录了每个扩展的 `sub`，所以 `ext add` 不会冲掉其他扩展之前的子配置。
- 状态里有、recipe 里没有的扩展或 feature → 报错，要求手工处理。

示例：

```text
install --ext curl=sub:curl[nghttp2]  → libcurl = [nghttp2]
ext add swoole --ext swoole=sub:curl[ssl]
                                       → libcurl = [nghttp2, ssl]
ext remove swoole                      → libcurl = [nghttp2]
```

## 5. 模拟边界

`install` / `update` / `ext add` / `ext remove` 只做：

1. 求解。
2. 更新 `state.json`。
3. 打印结果或计划。

不下载、不编译、不创建安装树。
