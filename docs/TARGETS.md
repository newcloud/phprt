# 构建目标（targets）

Targets 是产品本次要生成的构建目标，例如 PHP 的 CLI、FPM、CGI。它和另外两个概念
分开：

| 概念 | 例子 | 归谁 |
| --- | --- | --- |
| dependencies | openssl、libxml2、libzip | dep recipe + 组件 `needs` |
| features | libcurl 的 ssl、gd 的 jpeg | 组件 / 依赖 recipe |
| **targets** | PHP 的 cli、fpm、cgi | 产品 recipe |

Rust 只实现通用的解析、选择、校验和参数合并，不判断目标叫什么、对应什么二进制。

## 1. 命令行

```bash
# 只选 CLI
phprt install php@8.5 --targets="cli"

# 选择 CLI 和 FPM
phprt install php@8.5 --targets="cli,fpm"

# 为 FPM 传配置
phprt install php@8.5 --targets="cli,fpm=user:www-data,group:www-data"
```

解析结果：

```json
{
  "cli": {},
  "fpm": {
    "user": "www-data",
    "group": "www-data"
  }
}
```

语法与 `--component` 共用同一套切分规则：

- 顶层用逗号分隔，`[...]` 内的逗号不切；
- `名字` 或 `名字=key:value` 开始一个 target；
- 后续 `key:value` / `key:[a,b]` 属于当前 target；
- 同一 target、同一配置项重复出现直接报错；
- 未知 target、未知配置项、类型不匹配直接报错，不静默忽略。

`--targets` 可以重复，但同一个 target 只能出现一次。

顺序不影响构建身份：target 名和配置项名按字典序规范化；列表值保留元素顺序。
下面两条等价：

```bash
--targets="cli,fpm=user:www-data,group:www-data"
--targets="fpm=group:www-data,user:www-data,cli"
```

`--targets` 只在 `install`、`update`、`plan` 上接收。`add` / `remove` 沿用 state
里的选择；要改目标用 `install --targets`（已装版本需要 `--force`）。

## 2. 默认值

不传 `--targets` 时按以下优先级：

```text
CLI --targets > state.targets > bundle 的 [targets] default
```

bundle 示例：

```toml
# bundles/php/8.5.toml
[targets]
default = ["cli"]
```

- 默认值只写 target 名，不写配置。
- Product 没有声明 targets 时，`--targets` 或 `[targets] default` 都会报错。
- Product 声明了 targets，但 CLI、state、bundle 都没有选择时报错，不擅自挑第一个。
- 默认 target 必须在该系列的声明里，否则加载失败。

## 3. Recipe 声明

Targets 写在每个产品系列自己的 `products/<产品>/<系列>.star`，不写进
`_profile.star`，也不做共享常量。重复是允许的，版本差异直接在文件里可见。

```python
TARGETS = {
    "cli": {
        "flags": ["--enable-cli"],
        "disable_flags": ["--disable-cli"],
        "artifacts": ["bin/php"],
        "verify": ["{tree}/bin/php", "-v"],
        "verify_modules": "{tree}/bin/php -m",
        "entry": "myphp-{series}",
        "entry_template": "entry.sh",
    },
    "fpm": {
        "flags": ["--enable-fpm"],
        "disable_flags": [],
        "artifacts": ["sbin/php-fpm"],
        "verify": ["{tree}/sbin/php-fpm", "-v"],
        "config": {
            "user": {"type": "string", "flags": "--with-fpm-user={value}"},
            "group": {"type": "string", "flags": "--with-fpm-group={value}"},
        },
    },
}

TARGET = product(
    ...,
    targets = TARGETS,
    ...
)
```

字段：

| 字段 | 说明 |
| --- | --- |
| `flags` | target 选中时追加的 configure 参数 |
| `disable_flags` | target 未选中时追加的 configure 参数；不写就不生成 |
| `artifacts` | target 选中时追加的产物 |
| `post_install` | target 选中时追加的安装后动作 |
| `verify` | target 选中时执行的产品校验命令 |
| `verify_modules` | target 选中时执行的模块校验 shell 命令 |
| `entry` / `entry_template` | target 选中时生成的入口 |
| `config` | target 自己的配置项：名字 -> `{ type, flags }` |

规则：

- 选中的 target：应用 `flags`，不应用 `disable_flags`，允许它的 `config`。
- 未选中的 target：应用 `disable_flags`，不应用 `flags`，不接受配置。
- 没有声明 `disable_flags` 的 target 不会自动生成禁用参数。
- `disable_flags` 是构建参数，不是用户可选 feature。

`artifacts` / `verify` / `entry` / `post_install` 是可选的。产品声明了 targets 时：

- 产物 = 产品 `artifacts` + 选中 target 的 `artifacts`（去重）；
- 校验 = 每个选中 target 的 `verify` / `verify_modules`；
- 入口 = 选中 target 里声明了 `entry` 的那一个；多个同时选中时报错；
- 安装后动作 = 产品 profile 的 `post_install` + 选中 target 的 `post_install`。

这样 `--targets="fpm"` 不会再去执行产品级的 `bin/php -v`。

## 4. 配置项

```python
"config": {
    "user": {"type": "string", "flags": "--with-fpm-user={value}"},
    "tags": {"type": "list", "flags": "--with-tag={value}"},
}
```

- `type` 只支持 `string` 和 `list`。
- `flags` 必须包含 `{value}`。
- `string` 生成一条参数；`list` 对每个元素展开一次，按输入顺序。
- 只有选中的 target 能使用配置；未选中的 target 传配置会报未知配置项。
- 配置值、配置项名、target 名都参与构建身份。

## 5. State、scheme 与重建

state 记录实际选择：

```json
"targets": {
  "cli": {},
  "fpm": {
    "group": "www-data",
    "user": "www-data"
  }
}
```

`scheme` 导出：

```text
phprt install php@8.5 '...' --targets 'cli,fpm=group:www-data,user:www-data'
```

- `scheme` / `scheme --lock` 都导出有效 targets。
- marker 的 `scheme_cmd` 也记录 targets。
- targets 或配置变化会改变方案指纹；已装版本按现有 install 语义提示“已装方案与本次不同”，
  需要 `--force` 或删掉旧树重装，不会只改 state。
- 配置顺序、target 顺序不改变指纹。

## 6. PHP 的实际参数

PHP 的 targets 在每个 `products/php/<系列>.star` 里完整声明。已核对 8.3 / 8.4 / 8.5
的 configure：

- `--disable-cli`、`--disable-cgi` 存在；
- `--enable-fpm` 存在；
- `--disable-fpm` 不存在。FPM 默认关闭，所以 fpm 的 `disable_flags` 为空。

结果：

| 选择 | configure 参数 |
| --- | --- |
| `cli` | `--enable-cli --disable-cgi` |
| `cli,fpm` | `--enable-cli --enable-fpm --disable-cgi` |
| `cli,fpm=user:www-data,group:www-data` | 上面再加 `--with-fpm-group=www-data --with-fpm-user=www-data`（按配置名排序） |
| `fpm` | `--disable-cli --enable-fpm --disable-cgi` |
| `cgi` | `--disable-cli --enable-cgi` |

未选中的 FPM 不生成任何参数，因为上游没有对应的禁用参数。

## 7. 错误

```text
error: targets spec 里重复指定了 target cli
error: 本次选择里的 target nope 不受 product:php@8.5.12 支持（支持：cgi, cli, fpm）
error: 本次选择里的 target fpm 没有配置项 tags（支持：group, user）
error: 本次选择里的 target fpm 配置项 user 的类型是 string，不能传列表
error: product:nginx@1.29.8 没有声明 targets，不能使用 --targets 或 [targets] 默认值
error: php@8.6 声明了 targets，但 bundle 没有 [targets] default
```

## 8. 验收

```bash
cargo run -- plan php@8.5
cargo run -- plan php@8.5 --targets 'cli,fpm=user:www-data,group:www-data'
cargo run -- plan php@8.5 --targets 'fpm'
cargo run -- plan nginx@1.29 --targets 'cli'          # 必须失败
cargo run -- install php@8.5 --targets 'fpm' --downloads --offline
cargo run -- install php@8.5 --targets 'cli,fpm' --force
./.phprt/php-8.5.12/sbin/php-fpm -v
cargo run -- scheme php@8.5
```
