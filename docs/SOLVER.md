# 求解

## 1. 依赖边

`plan::children` 按目标类型生成边：

| 目标 | 子节点 |
| --- | --- |
| dep | `needs` 里的每个 `dep:*`，加上已启用 feature 的 `needs` |
| ext | `needs` 里的每个 `dep:*` |
| php | `exts` 里 **static** 的扩展 |

shared 扩展不进入 php 的子节点，改为在 php 之后单独展开，避免成环。

## 2. 拓扑顺序

`resolve` 做 DFS 后序输出，所以依赖总在消费者之前。

- 根目标默认是 php。
- 先访问根（php 只带 static 扩展），再按 `exts` 顺序把 shared 扩展逐个访问并追加。
  结果就是：static 依赖 → static 扩展 → php → shared 依赖 → shared 扩展。

`--ext` 只影响链接方式和子配置，不会把未列在 `php.exts` 里的扩展拉进闭包；
`ext add` 会把它写进状态文件，之后求解时以状态里的 `ext` 列表为准。

## 3. 静态 / 动态

- ext 的默认模式由 `origin` 决定：builtin → static，pecl → shared。
- 命令行 `link:` / `名字=模式` 覆盖默认值。
- 选择的模式必须出现在 ext 的 `link` 契约里。
- static：ext 的 configure 参数汇总进 PHP 的 configure 行。
- shared：ext 独立构建，追加一条 `php:<版本>` 依赖，排在 php 之后。

## 4. 子配置与 feature

两层要分清：

```text
扩展自己的子配置       dep 的 feature
sub:curl[ssl,nghttp2]  └──────┬──────┘
  │                            │
  └ 决定是否加              转给 dep，决定它怎么编
    --enable-swoole-curl
```

- `sub:<名字>`：打开扩展自己的子配置，并把它的 `configure` 追加到 ext。
- `[...]`：要转发给 `sub.dep` 的 feature，必须是该 dep 的 `features` 里的名字。
- 不写方括号：只开开关，不转发。
- 不写 `sub:<名字>`：开关不生效，也不转发。

示例：

```text
--ext swoole=sub:curl[ssl] --ext curl=link:shared,sub:curl[nghttp2]

ext:swoole   sub curl [ssl]     -> libcurl
ext:curl     sub curl [nghttp2] -> libcurl
dep:libcurl  features = nghttp2, ssl
```

curl 和 swoole 都不解释 `ssl` / `nghttp2` 怎么编；它们只把名字转给 libcurl，
具体参数和额外依赖由 libcurl 的 `features` 定义。

## 5. 并集与持久化

本次选择算出的集合：

```text
current(dep) = ∪ { 每个 ext 的被选中 sub 方括号里的 feature }
```

最终构建集合：

```text
build(dep) = current(dep) ∪ state.json 里的历史集合
```

- 同一次求解内，多个消费者的请求自动取并集。
- 跨命令用 `.phprt/php-<系列>/state.json` 累计；`ext add` 和 `install` 只增不减。
- `plan` 读取并展示并集，但不写；`install` / `update` / `ext add` 写入。
- `ext remove` 按剩余扩展重算 feature，是唯一允许收窄的动作。
- 状态里出现 recipe 没有的 feature → 报错，不静默删除。

这样第一次 `sub:curl[nghttp2]`、第二次 `sub:curl[ssl]` 之后，libcurl 始终按
`nghttp2 + ssl` 构建，不会被后来的消费者改小。

## 6. 错误

- 未知目标：`未知目标 <id>`。
- 裸名歧义：列出所有候选，要求写成 `kind:name`。
- 成环：DFS 遇回边时输出完整路径，例如 `dep:a -> dep:b -> dep:a`。
- 缺失依赖在加载阶段就报错，不会进入求解。
- 未知子配置、未知 feature 在 `--ext` 解析阶段报错。
