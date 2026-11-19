//! 产品构建目标（targets）的解析、选择、校验和规范化。
//!
//! 语法与组件参数一致：顶层用逗号分隔，`[...]` 内的逗号不切；target 自己的
//! 配置写成 `key:value`，第一个配置可以直接跟在 `名字=` 后面。
//!
//! ```text
//! cli
//! cli,fpm
//! cli,fpm=user:www-data,group:www-data
//! ```

use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};

use crate::recipe::{Registry, TargetSelection, TargetValue, split_top};

/// 解析 `--targets` 的原始字符串。
///
/// 同一 target 出现两次、同一配置项出现两次都直接报错，不合并。
pub fn parse_cli(specs: &[String]) -> Result<TargetSelection> {
    let mut out = TargetSelection::new();
    for spec in specs {
        let mut current: Option<String> = None;
        for raw in split_top(spec, ',')? {
            let part = raw.trim();
            if part.is_empty() {
                bail!("targets spec {spec} 里有空的片段");
            }

            if let Some((name, rest)) = part.split_once('=') {
                let name = name.trim();
                if name.is_empty() {
                    bail!("targets spec {part} 缺少 target 名");
                }
                if out.contains_key(name) {
                    bail!("targets spec 里重复指定了 target {name}");
                }
                if rest.trim().is_empty() {
                    bail!("target {name} 的配置是空的");
                }
                if !rest.contains(':') {
                    bail!(
                        "target {name} 的配置 {rest} 必须写成 key:value；\
                         多个配置用逗号分隔"
                    );
                }
                out.insert(name.to_owned(), BTreeMap::new());
                current = Some(name.to_owned());
                add_config(&mut out, name, rest)?;
            } else if part.contains(':') {
                let Some(name) = current.as_deref() else {
                    bail!("targets spec {spec} 的第一个片段不能是配置项 {part}");
                };
                add_config(&mut out, name, part)?;
            } else {
                if out.contains_key(part) {
                    bail!("targets spec 里重复指定了 target {part}");
                }
                out.insert(part.to_owned(), BTreeMap::new());
                current = Some(part.to_owned());
            }
        }
    }
    Ok(out)
}

fn add_config(out: &mut TargetSelection, target: &str, text: &str) -> Result<()> {
    let Some((key, value)) = text.split_once(':') else {
        bail!("target {target} 的配置 {text} 必须写成 key:value");
    };
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() {
        bail!("target {target} 的配置缺少名字");
    }
    if value.is_empty() {
        bail!("target {target} 的配置项 {key} 是空的");
    }
    let parsed = parse_value(target, key, value)?;
    let config = out
        .get_mut(target)
        .ok_or_else(|| anyhow!("内部错误：target {target} 不存在"))?;
    if config.insert(key.to_owned(), parsed).is_some() {
        bail!("target {target} 里重复指定了配置项 {key}");
    }
    Ok(())
}

fn parse_value(target: &str, key: &str, value: &str) -> Result<TargetValue> {
    if let Some(inner) = value.strip_prefix('[') {
        let Some(inner) = inner.strip_suffix(']') else {
            bail!("target {target} 的配置项 {key} 的列表缺少 ]");
        };
        if inner.trim().is_empty() {
            bail!("target {target} 的配置项 {key} 的列表是空的");
        }
        let mut items = Vec::new();
        for raw in split_top(inner, ',')? {
            let item = raw.trim();
            if item.is_empty() {
                bail!("target {target} 的配置项 {key} 的列表里有空值");
            }
            items.push(item.to_owned());
        }
        Ok(TargetValue::Many(items))
    } else {
        Ok(TargetValue::One(value.to_owned()))
    }
}

/// 计算本次使用的 targets：CLI > state > bundle 默认值。
///
/// Product 没有声明 targets 时，任何非空来源都是错误；声明了 targets 但三个来源
/// 都为空也报错，不擅自挑一个。
pub fn effective(
    reg: &Registry,
    bundle_default: &[String],
    state: &TargetSelection,
    cli: Option<&TargetSelection>,
) -> Result<TargetSelection> {
    let product = reg.product_any()?;
    let cli_is_set = cli.is_some_and(|selection| !selection.is_empty());

    if product.targets.is_empty() {
        if cli_is_set || !state.is_empty() || !bundle_default.is_empty() {
            bail!(
                "{} 没有声明 targets，不能使用 --targets 或 [targets] 默认值",
                product.id()
            );
        }
        return Ok(TargetSelection::new());
    }

    let selection = if let Some(cli) = cli {
        if cli.is_empty() {
            bail!("--targets 不能是空选择");
        }
        cli.clone()
    } else if !state.is_empty() {
        state.clone()
    } else if !bundle_default.is_empty() {
        bundle_default
            .iter()
            .map(|name| (name.clone(), BTreeMap::new()))
            .collect()
    } else {
        bail!(
            "{} 声明了 targets，但 bundle 没有 [targets] default；\
             用 --targets 显式选择，或在 bundles/<产品>/<系列>.toml 里补默认值",
            product.id()
        );
    };

    validate_selection(reg, &selection, "本次选择")?;
    Ok(selection)
}

/// 校验选择里的 target 名和配置项都符合当前系列的声明。
pub fn validate_selection(reg: &Registry, selection: &TargetSelection, source: &str) -> Result<()> {
    let product = reg.product_any()?;
    let available = names(&product.targets.keys().cloned().collect::<Vec<_>>());
    for (name, config) in selection {
        let Some(target) = product.targets.get(name) else {
            bail!(
                "{source}里的 target {name} 不受 {} 支持（支持：{available}）",
                product.id()
            );
        };
        for (key, value) in config {
            let Some(decl) = target.config.get(key) else {
                let keys: Vec<String> = target.config.keys().cloned().collect();
                bail!(
                    "{source}里的 target {name} 没有配置项 {key}（支持：{}）",
                    names(&keys)
                );
            };
            match (decl.kind.as_str(), value) {
                ("string", TargetValue::One(value)) => {
                    if value.is_empty() {
                        bail!("{source}里的 target {name} 配置项 {key} 是空值");
                    }
                }
                ("list", TargetValue::Many(values)) => {
                    if values.is_empty() || values.iter().any(String::is_empty) {
                        bail!("{source}里的 target {name} 配置项 {key} 的列表里有空值");
                    }
                }
                ("string", TargetValue::Many(_)) => {
                    bail!("{source}里的 target {name} 配置项 {key} 的类型是 string，不能传列表")
                }
                ("list", TargetValue::One(_)) => {
                    bail!("{source}里的 target {name} 配置项 {key} 的类型是 list，需要写成 [a,b]")
                }
                (other, _) => {
                    bail!("{source}里的 target {name} 配置项 {key} 的类型 {other} 不合法")
                }
            }
        }
    }
    Ok(())
}

/// 把选择注入当前产品目标，供 plan / build / state 使用。
pub fn apply(reg: &mut Registry, selection: &TargetSelection) -> Result<()> {
    let product_id = reg.product_any()?.id();
    let product = reg
        .targets
        .get_mut(&product_id)
        .expect("product 目标刚刚检查过");
    product.selected_targets = selection.clone();
    Ok(())
}

/// 规范化后的 `--targets` 参数内容（不含 `--targets` 前缀）。
pub fn canonical(selection: &TargetSelection) -> String {
    let mut specs = Vec::new();
    for (name, config) in selection {
        if config.is_empty() {
            specs.push(name.clone());
            continue;
        }
        let mut parts = Vec::new();
        for (key, value) in config {
            match value {
                TargetValue::One(value) => parts.push(format!("{key}:{value}")),
                TargetValue::Many(values) => {
                    parts.push(format!("{key}:[{}]", values.join(",")));
                }
            }
        }
        specs.push(format!("{name}={}", parts.join(",")));
    }
    specs.join(",")
}

/// scheme 命令里的 `--targets '...'` 片段；空选择返回空串。
pub fn scheme_args(selection: &TargetSelection) -> String {
    if selection.is_empty() {
        String::new()
    } else {
        format!(" --targets '{}'", canonical(selection))
    }
}

/// 构建指纹用的规范化文本；target / 配置名排序，列表保留元素顺序。
pub fn digest_text(selection: &TargetSelection) -> String {
    let mut out = String::new();
    for (name, config) in selection {
        out.push_str(&format!("target={name}\n"));
        for (key, value) in config {
            let value = serde_json::to_string(value).unwrap_or_default();
            out.push_str(&format!("config={key}={value}\n"));
        }
    }
    out
}

/// plan 文本里的简短展示。
pub fn display(selection: &TargetSelection) -> String {
    if selection.is_empty() {
        "无".to_owned()
    } else {
        canonical(selection)
    }
}

fn names(items: &[String]) -> String {
    if items.is_empty() {
        "无".to_owned()
    } else {
        items.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(spec: &str) -> TargetSelection {
        parse_cli(&[spec.to_owned()]).unwrap()
    }

    #[test]
    fn parses_single_target() {
        let selection = parse("cli");
        assert!(selection.contains_key("cli"));
        assert!(selection["cli"].is_empty());
    }

    #[test]
    fn parses_multiple_targets() {
        let selection = parse("cli,fpm");
        assert_eq!(
            selection.keys().cloned().collect::<Vec<_>>(),
            ["cli", "fpm"]
        );
    }

    #[test]
    fn parses_target_config() {
        let selection = parse("cli,fpm=user:www-data,group:www-data");
        assert_eq!(
            selection["fpm"]["user"],
            TargetValue::One("www-data".to_owned())
        );
        assert_eq!(
            selection["fpm"]["group"],
            TargetValue::One("www-data".to_owned())
        );
    }

    #[test]
    fn parses_lists_and_internal_commas() {
        let selection = parse("fpm=tags:[a,b],user:www-data");
        assert_eq!(
            selection["fpm"]["tags"],
            TargetValue::Many(vec!["a".to_owned(), "b".to_owned()])
        );
        assert_eq!(
            selection["fpm"]["user"],
            TargetValue::One("www-data".to_owned())
        );
    }

    #[test]
    fn rejects_duplicate_target() {
        let err = parse_cli(&["cli,cli".to_owned()]).unwrap_err().to_string();
        assert!(err.contains("重复指定了 target cli"), "{err}");
    }

    #[test]
    fn rejects_duplicate_config() {
        let err = parse_cli(&["fpm=user:a,user:b".to_owned()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("重复指定了配置项 user"), "{err}");
    }

    #[test]
    fn rejects_config_without_target() {
        let err = parse_cli(&["user:www-data".to_owned()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("第一个片段不能是配置项"), "{err}");
    }

    #[test]
    fn canonical_is_order_independent() {
        let a = parse("cli,fpm=user:www-data,group:www-data");
        let b = parse("fpm=group:www-data,user:www-data,cli");
        assert_eq!(canonical(&a), canonical(&b));
        assert_eq!(digest_text(&a), digest_text(&b));
    }
}
