# 命令行

```bash
phprt [--recipes <目录>] [--root <目录>] [--ext <SPEC>] <子命令>
```

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `--recipes` | `recipes` | recipe 目录 |
| `--root` | `.` | 项目根目录；状态文件写在 `<root>/.phprt` 下 |
| `--ext` | 无 | 扩展选择，语法见下 |

子命令：

| 命令 | 说明 | 写状态文件 |
| --- | --- | --- |
| `list` | 列出所有目标及关键字段 | 否 |
| `graph` | 按当前选择打印依赖边 | 否 |
| `plan [目标] [--json]` | 求解并输出模拟计划 | 否 |
| `install` | 模拟安装：输出计划并写状态 | 是 |
| `update` | 模拟更新后重新求解 | 是 |
| `ext list [--installed\|--pending]` | 列出全部 / 已安装 / 待安装扩展 | 否 |
| `ext add <名字>` | 加入构建方案（待安装） | 是 |
| `ext remove <名字>` | 从方案移除，并重算 feature | 是 |

目标可以写完整 id（`dep:openssl`、`ext:curl`、`php:8.5.12`），也可以只写名字；
只写名字有歧义时报错并列出候选。

## ext list

```bash
phprt ext list              # 所有 recipe 里的扩展
phprt ext list --installed  # 只列已安装
phprt ext list --pending    # 只列已加入方案、未安装
```

状态取值：

| 状态 | 含义 |
| --- | --- |
| `installed` | 在方案里，且已安装（模拟） |
| `pending` | 在方案里，但还没安装 |
| `available` | recipe 存在，但不在方案里 |

```text
name         origin    link            status
curl         builtin   static,shared   installed
swoole       pecl      shared          pending
xdebug       pecl      shared          available
```

`--installed` 和 `--pending` 互斥。

没有状态文件时，PHP recipe 的 `exts` 是默认方案，这些扩展显示为 `pending`；
不在默认方案里、也没有被 `ext add` 加进来的扩展才是 `available`。

## `--ext` 语法

```
--ext <spec>[,<spec>...]        # 可以重复，每次一个 spec
spec := 名字
      | 名字=link:static|shared
      | 名字:link:static|shared
      | 名字=link:模式,sub:子配置
      | 名字=link:模式,sub:子配置[feature,feature]
      | 名字=sub:子配置
      | 名字=sub:子配置[feature,feature]
```

解析规则：

1. `--ext` 可以写多次，每次一个 spec。
2. 按逗号切分，但 `[...]` 里的逗号不切；`link:` / `sub:` 开头的片段并入前一个扩展。
3. 名字后面的 `=` 和 `:` 等价：`curl=link:shared` 和 `swoole:link:shared`。
4. 裸名字使用默认值：`origin=builtin` → static，`origin=pecl` → shared。
5. 只支持 `link:` / `sub:` 选项；`名字=shared` 这种简写不接受。
6. `sub:名字` 打开扩展自己的子配置，并应用它的 `configure`。
7. `sub:名字[a,b]` 在打开开关的同时，把 `a`、`b` 转给该子配置绑定的 dep。
   feature 之间也可以用 `+`。
8. 不写方括号 → 只打开开关，不向依赖传 feature。
9. 完全不写某个 `sub:` → 扩展不启用这个子配置，也不碰依赖。
10. 同一个扩展只能出现一次；同一个子配置也只能出现一次。

示例：

```bash
# swoole 打开 curl 支持，并把 ssl/nghttp2 转给 libcurl；
# curl 编成 shared，只把 ssl 转给 libcurl
phprt plan --ext swoole=sub:curl[ssl,nghttp2] --ext curl=link:shared,sub:curl[ssl]

# swoole 不写 sub:curl 时，不会加 --enable-swoole-curl
phprt plan --ext swoole=link:shared
```

## 状态文件

状态写在 `.phprt/php-<系列>/state.json`，例如 `.phprt/php-8.5/state.json`。
`plan` / `graph` / `ext list` 只读；`install` / `update` / `ext add` / `ext remove` 写入。
详见 [STATE.md](STATE.md)。

```bash
phprt install --ext curl=sub:curl[nghttp2]
phprt ext add swoole
phprt ext remove swoole
phprt ext list --installed
phprt update
```

## 校验与退出

`--ext` 在解析阶段就对照契约检查，按从左到右的顺序失败即停：

| 情况 | 错误示例 |
| --- | --- |
| 未知扩展 | `未知扩展 ext:nope` |
| 契约不支持的链接方式 | `ext:xdebug 契约不支持 static（支持：shared）` |
| 扩展没有这个子配置 | `ext:curl 契约里没有子配置 nope` |
| 依赖没有这个 feature | `dep:libcurl 契约没有 feature brotli` |
| 重复指定扩展 | `--ext 里重复指定了扩展 curl` |
| 未知选项 | `--ext curl 里未知选项 shared（只支持 link: / sub:）` |
| 状态文件里有未知 feature | `state.json 里 dep:libcurl 没有 feature brotli` |
| 移除不在方案里的扩展 | `ext:nope 不在构建方案里` |

任何错误都退出码 1，不输出计划内容。`plan` / `install` 成功时退出码 0。

## 目标位置

`plan` 的目标是位置参数：

```bash
phprt plan php:8.5.12 --ext openssl=link:shared
```
