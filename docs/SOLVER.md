# 求解

求解分两段：先按 PHP 系列解析版本线，再调用 recipe 得到具体 Target，最后跑原来的
依赖闭包和拓扑。

## 1. 版本线解析

输入：

```text
php 系列         install 的位置参数 / --php / 唯一 state
bundle           该系列扩展默认线 + [deps] 依赖库默认线 + ext_presets + default_exts
CLI --ext version: / --dep
state.bindings   之前显式保存的线
catalog          每条线的最新补丁
recipe LINES         扩展自己的线支持的 PHP、依赖约束
recipe PHP_LINES     builtin 扩展对同名依赖库的 PHP 约束
recipe PHP_DEP_LINES builtin 扩展对不同名依赖库的 PHP 约束（如 gd）
php recipe           builtin_exts / builtin_disable：该版本的内置扩展事实
```

优先级：

```text
CLI 显式 > state.bindings > bundle 默认 > 单线组件自动
```

对每个组件：

1. 如果显式指定了线，必须在 catalog 里。
2. 否则使用 bundle：有独立版本线的扩展取顶层键，依赖库取 `[deps]`；
   bundle 没有且只有一条线时直接用它。
3. 有自己版本线的扩展（swoole / xdebug）校验 `LINES[线].php` 包含当前 PHP 系列。
   - 显式线不满足 → 报错。
   - bundle 线不满足、且只有一条候选 → 自动改用并打印提示。
   - 否则报错并列出候选。
4. dep（openssl / libcurl / nghttp2）不声明 PHP 兼容性；它的线由 bundle、CLI
   或消费扩展的约束决定。

## 2. 依赖线约束

约束来自被选中的扩展，而不是 dep：

1. 扩展自己的版本线里，除 `php` 外的 key：

```python
LINES = {
    "4.8": {"php": ["7.4", "8.0", "8.2"], "openssl": ["1.1.1"]},
}
```

2. builtin 扩展的 `PHP_LINES`：

```python
# ext/openssl.star
PHP_LINES = {
    "8.5": ["1.1.1", "3.0", "3.5", "4.0"],
}
```

3. builtin 扩展对不同名依赖的 `PHP_DEP_LINES`：

```python
# ext/gd.star
PHP_DEP_LINES = {
    "8.5": {
        "libpng": ["1.6"],
        "jpeg": ["3.1"],
        "webp": ["1.6"],
        "avif": ["1.3"],
        "freetype": ["2.13"],
    },
}
```

对每个被约束的依赖：

```text
allowed = catalog 里该依赖的所有线
        ∩ 每个已选扩展允许的线
```

dep 自身不参与 PHP 兼容；它只是库。

- 显式 pin 不在 `allowed` → 报错。
- bundle 默认线在 `allowed` → 用它。
- bundle 默认线不在、但 `allowed` 只有一个 → 自动改用并打印提示。
- `allowed` 有多个或为空 → 报错，提示用 `--dep` 指定。

例：

```text
php8.2 bundle [deps]: openssl = 3.5
--ext swoole=version:4.8
swoole 4.8 只允许 openssl 1.1.1
→ openssl 自动改用 1.1.1，并打印提示
```

如果同时 `--dep openssl=3.5`，3.5 是显式 pin，直接报冲突。

## 3. 取补丁

选好线后从 catalog 取：

```text
swoole.6.2 → version 6.2.5 / url / sha256
openssl.3.5 → version 3.5.9 / url / sha256
```

补丁号不会出现在 recipe 或 bundle 里；安全更新只改 catalog。

## 4. 调用 recipe

Rust 构造 ctx：

```text
php_series / php_version
bindings
deps.<名字> = {line, version, url, sha256}
self = 当前 recipe 同名组件的记录
```

然后：

- `TARGET` 是函数 → 调用 `TARGET(ctx)`。
- `TARGET` 是 dict → 直接使用。
- php recipe 的 version/url/sha256 由 catalog 注入。
- ext 的 origin 不写在 recipe：由 php recipe 的 `builtin_exts` 推导，
  并通过 `ctx["is_builtin"]` 注入给 `target(ctx)`。

### 4.1 默认方案

```text
无 state：方案 = bundles/php<系列>.toml 的 [default_exts]
                 - --disable-default-ext
                 + --ext
有 state：方案 = state.ext
                 - --disable-default-ext
                 + --ext
```

- `default_exts` 是 TOML 表：`link` / `features` / `deps`，不允许 `version:`。
- 默认扩展必须是当前 PHP 系列 `builtin_exts` 的子集，并且有 ext recipe。
- `default_exts` 可用 `ref = "ext_presets.<同名扩展>"` 引用 `[ext_presets]`；
  写 ref 时不能再写本地 `features` / `deps`，`link` 仍可本地覆盖。
- `[ext_presets]` 只给被选中的扩展提供默认配置，不加入默认方案。
- 选中扩展时的优先级：CLI 显式 `feature:[...]`（含 `[]`） > state > preset > 空。
- 首次 install / ext add / ext remove 会把默认集和 preset materialize 到 state；
  已有 state 不随 `default_exts` / `ext_presets` 变化。
- 不做依赖展开：有依赖的内置扩展要在 `default_exts` 里一起写。
- 条件 feature 在当前 PHP / 依赖线下不可用时提示并丢弃；显式 `--ext` 选择才报错。

### 4.2 PHP configure 参数

```text
base = php recipe 的 configure

for (name, flag) in php.builtin_disable:
    if name 在方案里且 link=static:
        跳过 flag
    else:
        base += flag

for ext in 方案里的 static 扩展:
    base += ext.configure + 选中 sub 的 configure
```

- `builtin_disable` 是 PHP 版本事实；ext recipe 不写关闭参数。
- 同一个内置扩展只会出现正向或关闭参数其中一组。
- 没有 recipe 的内置扩展（如 libxml/dom）只会出现关闭参数。

## 5. 依赖边

`plan::children` 按目标类型生成边：

| 目标 | 子节点 |
| --- | --- |
| dep | `needs` 里的每个 `dep:*`，加上已启用 feature 的 `needs` |
| ext | `needs` 里的每个 `dep:*`，加上 `tools` 里的每个 `dep:*` |
| php | 构建方案（`exts`）里 **static** 的扩展 |

shared 扩展不进入 php 的子节点，改为在 php 之后单独展开，避免成环。
`tools` 只影响构建顺序和 `PATH`，不参与 include/lib 计算。

## 6. 拓扑顺序

`resolve` 做 DFS 后序输出，所以依赖总在消费者之前。

- 根目标默认是当前 PHP。
- 先访问根（php 只带 static 扩展），再按 `exts` 顺序把 shared 扩展逐个访问并追加。
  结果就是：static 依赖 → static 扩展 → php → shared 依赖 → shared 扩展。

## 7. feature 并集

本次选择算出的集合：

```text
current(dep) = ∪ { 每个 ext 的被选中 sub 方括号里的 feature }
```

最终构建集合：

```text
build(dep) = current(dep) ∪ state.json 里的历史集合
```

- 同一次求解内，多个消费者的请求自动取并集。
- 跨命令用 state 累计；`ext add` / `install` / `update` 只增不减。
- `ext remove` 按剩余扩展重算，是唯一允许收窄的动作。
- `plan` / `graph` 读取并展示并集，但不写。

## 8. 错误

- 未知目标：`未知目标 <id>`。
- 裸名歧义：列出所有候选，要求写成 `kind:name`。
- 成环：DFS 遇回边时输出完整路径，例如 `dep:a -> dep:b -> dep:a`。
- 版本线不存在、PHP 不支持、依赖线冲突：在加载 recipe 之前报错，不输出半份计划。
