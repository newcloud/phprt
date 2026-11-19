# 求解

求解分两段：先按 `产品@系列` 解析版本线，再调用 recipe 得到具体 Target，最后跑依赖
闭包和拓扑。

## 1. 版本线解析

输入：

```text
产品 + 系列       命令行目标 php@8.5
bundle           该系列组件默认线 + [deps] 依赖库默认线 + component_presets + default_components
CLI --component version: / --dep
state.bindings   之前显式保存的线
catalog          每条线的最新补丁
recipe LINES          组件自己的线支持哪些产品、依赖约束；产品自己也可以约束依赖
recipe PRODUCT_LINES  bundled 组件对同名依赖库的约束
recipe PRODUCT_DEP_LINES bundled 组件对不同名依赖库的约束（如 gd）
product recipe        bundled / bundled_disable：该版本的内置组件事实
```

优先级：

```text
CLI 显式 > state.bindings > bundle 默认 > 单线组件自动
```

对每个组件：

1. 如果显式指定了线，必须在 catalog 里。
2. 否则使用 bundle：有独立版本线的组件取顶层键，依赖库取 `[deps]`；
   bundle 没有且只有一条线时直接用它。
3. 有自己版本线的组件（swoole / xdebug / redis / lua-nginx-module / …）校验
   `LINES[线].product` 包含当前产品系列。
   - 显式线不满足 → 报错。
   - bundle 线不满足 → 报错并列出可用线，不再自动改用；错误会点名
     `bundles/<产品>/<系列>.toml`，方便改正写错的默认线。
   - 只有一条 catalog 线的组件被自动绑定时不支持当前系列 → 直接跳过，
     该系列不加载这个组件（例如 protobuf 5.36 最低支持 PHP 8.2）；
     显式 `--component` 选中它时会报「不支持产品 <系列>」。
4. dep（openssl / libcurl / nghttp2）不声明产品兼容性；它的线由 bundle、CLI
   或消费组件 / 产品的约束决定。

## 2. 依赖线约束

约束来自产品 recipe 和被选中的组件，而不是 dep：

1. 产品自己的 `LINES[系列]`：

```python
LINES = {
    "1.29": {"openssl": ["3.0", "3.5", "4.0"], "pcre2": ["10.49"]},
}
```

2. 组件自己的版本线里，除 `product` 外的 key：

```python
LINES = {
    "4.8": {"product": ["7.4", "8.0", "8.2"], "openssl": ["1.1.1"]},
}
```

3. bundled 组件的 `PRODUCT_LINES`：

```python
# components/php/openssl.star
PRODUCT_LINES = {
    "8.5": ["1.1.1", "3.0", "3.5", "4.0"],
}
```

4. bundled 组件对不同名依赖的 `PRODUCT_DEP_LINES`：

```python
# components/php/gd.star
PRODUCT_DEP_LINES = {
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
        ∩ 产品允许的线
        ∩ 每个已选组件允许的线
```

dep 自身不参与产品兼容；它只是库。

- 显式 pin 不在 `allowed` → 报错。
- bundle 默认线在 `allowed` → 用它。
- bundle 默认线不在 `allowed` → **报错**，不会自动换线；错误点名 bundle 文件和可用的线。
- bundle 和 CLI 的线行为一致：写出来的线就是硬要求，冲突时都必须显式改。
- 只有完全没有声明（bundle 没写、CLI 没给）且 `allowed` 恰好一条时才自动使用，
  并打印提示；`allowed` 有多个或为空 → 报错，提示用 `--dep` 指定。

例：

```text
php 8.2 bundle [deps]: openssl = 3.5
--component swoole=version:4.8
swoole 4.8 只允许 openssl 1.1.1
→ error: bundle 里 openssl = 3.5 与当前产品/组件选择冲突（可用：1.1.1）

# 要换线必须显式写：
phprt plan php@8.2 --component swoole=version:4.8 --dep openssl=1.1.1
```

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
product / product_series / product_version
bindings
deps.<名字> = {line, version, url, sha256}
self = 当前 recipe 同名组件的记录
is_bundled = 该组件是否在产品 recipe 的 bundled 列表里
```

然后：

- `TARGET` 是函数 → 调用 `TARGET(ctx)`。
- `TARGET` 是 dict → 直接使用。
- product recipe 必须是 dict；version/url/sha256 由 catalog 注入。
- 组件的 source（bundled / external）由产品 recipe 的 `bundled` 推导，
  并通过 `ctx["is_bundled"]` 注入给 `target(ctx)`。

### 4.1 默认方案

```text
无 state：方案 = bundles/<产品>/<系列>.toml 的 [default_components]
                 - --disable-default-component
                 + --component
有 state：方案 = state.components
                 - --disable-default-component
                 + --component
```

- `default_components` 是 TOML 表：`mode` / `features` / `deps`，不允许 `version:`。
- 默认组件必须是当前产品 `bundled` 的子集，并且有 component recipe。
- `default_components` 可用 `ref = "component_presets.<同名组件>"` 引用
  `[component_presets]`；写 ref 时不能再写本地 `features` / `deps`，`mode` 仍可本地覆盖。
- `[component_presets]` 只给被选中的组件提供默认配置，不加入默认方案。
- 选中组件时的优先级：CLI 显式 `feature:[...]`（含 `[]`） > state > preset > 空。
- 首次 install / add / remove 会把默认集和 preset materialize 到 state；
  已有 state 不随 `default_components` / `component_presets` 变化。
- 不做依赖展开：有依赖的 bundled 组件要在 `default_components` 里一起写。
- 条件 feature 在当前产品 / 依赖线下不可用时提示并丢弃；显式 `--component` 选择才报错。

### 4.2 产品 configure 参数

```text
base = product recipe 的 configure

for (name, flag) in product.bundled_disable:
    if name 在方案里且 strategy=fold:
        跳过 flag
    else:
        base += flag

for component in 方案里的 fold 组件:
    base += component.configure
          + 选中 feature / dep 的 configure
          + profile.modes[mode].configure（如 --add-module=...）
```

- `bundled_disable` 是产品版本事实；组件 recipe 不写关闭参数。
- 同一个 bundled 组件只会出现正向或关闭参数其中一组。
- 没有 recipe 的 bundled 组件（如 libxml/dom）只会出现关闭参数。

## 5. 依赖边

`plan::children` 按目标类型生成边：

| 目标 | 子节点 |
| --- | --- |
| dep | `needs` 里的每个 `dep:*`，加上已启用 feature 的 `needs` |
| component | `needs` 里的每个 `dep:*`；`feature` 的 needs；standalone 时加 `tools` |
| product | `needs` 里的每个 `dep:*`，加上方案里 **fold** 的组件 |

standalone 组件不进入产品的子节点，改为在产品之后单独展开，避免成环。
`tools` 只影响构建顺序和 `PATH`，不参与 include/lib 计算。

## 6. 拓扑顺序

`resolve` 做 DFS 后序输出，所以依赖总在消费者之前。

- 根目标默认是当前产品。
- 先访问根（产品只带 fold 组件），再按 `components` 顺序把 standalone 组件逐个访问并追加。
  结果就是：依赖 → fold 组件 → 产品 → standalone 组件。

## 7. feature 并集

本次选择算出的集合：

```text
current(dep) = ∪ { 每个 component 的被选中 dep 方括号里的 feature }
             ∪ 产品 deps 里声明的 feature
```

最终构建集合：

```text
build(dep) = current(dep) ∪ state.json 里的历史集合
```

- 同一次求解内，多个消费者的请求自动取并集。
- 跨命令用 state 累计；`add` / `install` / `update` 只增不减。
- `remove` 按剩余组件重算，是唯一允许收窄的动作。
- `plan` / `graph` 读取并展示并集，但不写。

## 8. 错误

- 未知目标：`未知目标 <id>`。
- 裸名歧义：列出所有候选，要求写成 `kind:name`；产品节点写 `product:<名字>@<版本>`。
- 成环：DFS 遇回边时输出完整路径，例如 `dep:a -> dep:b -> dep:a`。
- 版本线不存在、产品不支持、依赖线冲突：在加载 recipe 之前报错，不输出半份计划。
