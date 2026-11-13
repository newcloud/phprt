use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};

use crate::bundle::Bundle;
use crate::catalog::{Artifact, Catalog};
use crate::recipe::{Kind, RecipeMeta};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// CLI 或 state 里显式指定，冲突时必须报错。
    Pinned,
    /// bundle 里的默认线，冲突且只有一个候选时可以自动改。
    Bundle,
    /// 组件只有一条线。
    Single,
    /// 约束求交后自动选出的唯一线。
    Auto,
}

#[derive(Debug, Clone)]
struct Choice {
    line: String,
    source: Source,
}

/// 一次求解使用的版本线绑定。
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    pub lines: BTreeMap<String, String>,
    pub artifacts: BTreeMap<String, Artifact>,
    pub notes: Vec<String>,
}

pub fn resolve(
    catalog: &Catalog,
    bundle: &Bundle,
    metas: &[RecipeMeta],
    php_series: &str,
    selected_exts: &[String],
    pins: &BTreeMap<String, String>,
) -> Result<Bindings> {
    // 1. 显式指定的线必须存在。
    for (name, line) in pins {
        let lines = catalog
            .lines(name)
            .ok_or_else(|| anyhow::anyhow!("「{name}」没有可选版本（catalog 里没有这个组件）"))?;
        if !lines.contains_key(line) {
            bail!(
                "「{name}」没有版本线 {line}（可用：{}）",
                lines.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        }
    }

    for (name, line) in &bundle.ext_lines {
        let Some(meta) = ext_meta(metas, name) else {
            bail!("bundle 里的 {name} 不是扩展");
        };
        if meta.lines.is_empty() {
            if metas
                .iter()
                .any(|meta| meta.kind == Kind::Dep && meta.name == *name)
            {
                bail!(
                    "bundle 里的 {name} 是内置扩展，没有自己的版本线；\
                     依赖库 {name} 的默认线请写进 [deps]"
                );
            }
            bail!("bundle 里的 {name} 是内置扩展，没有自己的版本线");
        }
        let Some(lines) = catalog.lines(name) else {
            bail!("bundle 里的 {name} 不在 catalog 中");
        };
        if !lines.contains_key(line) {
            bail!(
                "bundle 里的 {name} {line} 不在 catalog（可用：{}）",
                lines.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        }
    }
    for (name, line) in &bundle.dep_lines {
        let is_dep = metas
            .iter()
            .any(|meta| meta.kind == Kind::Dep && meta.name == *name);
        if !is_dep {
            if ext_meta(metas, name).is_some() {
                bail!("bundle [deps] 里的 {name} 是扩展，默认线请写在顶层");
            }
            bail!("bundle [deps] 里的 {name} 不是依赖库");
        }
        let Some(lines) = catalog.lines(name) else {
            bail!("bundle [deps] 里的 {name} 不在 catalog 中");
        };
        if !lines.contains_key(line) {
            bail!(
                "bundle [deps] 里的 {name} {line} 不在 catalog（可用：{}）",
                lines.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        }
    }
    for name in bundle.ext_lines.keys() {
        if bundle.dep_lines.contains_key(name) {
            bail!("bundle 里的 {name} 同时写在了顶层和 [deps]");
        }
    }
    let mut defaults = bundle.ext_lines.clone();
    defaults.extend(bundle.dep_lines.clone());

    // 2. 初始选择：pins > bundle > 唯一的线。
    let mut choices: BTreeMap<String, Choice> = BTreeMap::new();
    for name in catalog.components.keys().filter(|name| *name != "php") {
        let lines = catalog.lines(name).expect("catalog 刚刚校验过");
        let choice = if let Some(line) = pins.get(name) {
            Some(Choice {
                line: line.clone(),
                source: Source::Pinned,
            })
        } else if let Some(line) = defaults.get(name) {
            Some(Choice {
                line: line.clone(),
                source: Source::Bundle,
            })
        } else if lines.len() == 1 {
            Some(Choice {
                line: lines.keys().next().expect("刚刚判断只有一条").clone(),
                source: Source::Single,
            })
        } else {
            None
        };
        if let Some(choice) = choice {
            if !lines.contains_key(&choice.line) {
                let source = if choice.source == Source::Pinned {
                    "显式指定"
                } else {
                    "bundle 默认"
                };
                bail!(
                    "{name} 的{source}线 {} 不在 catalog（可用：{}）",
                    choice.line,
                    lines.keys().cloned().collect::<Vec<_>>().join(", ")
                );
            }
            choices.insert(name.clone(), choice);
        }
    }

    // 3. 每一条已绑定的线都要满足它自己的 PHP 兼容表。
    let notes = {
        let mut notes = Vec::new();
        for (name, choice) in choices.clone() {
            let Some(meta) = component_meta(metas, &name) else {
                continue;
            };
            if meta.lines.is_empty() {
                continue;
            }
            let allowed = allowed_lines(catalog, meta, php_series);
            if allowed.contains(&choice.line) {
                continue;
            }
            if choice.source == Source::Pinned {
                bail!(
                    "{name} {} 不支持 PHP {php_series}（可用：{}）",
                    choice.line,
                    list(&allowed)
                );
            }
            if choice.source == Source::Bundle && allowed.len() == 1 {
                let line = allowed.iter().next().expect("刚刚判断只有一个").clone();
                notes.push(format!(
                    "{name} 的默认线 {} 不适用于 PHP {php_series}，自动改用 {line}",
                    choice.line
                ));
                choices.insert(
                    name.clone(),
                    Choice {
                        line,
                        source: Source::Auto,
                    },
                );
                continue;
            }
            bail!(
                "{name} {} 不支持 PHP {php_series}（可用：{}）",
                choice.line,
                list(&allowed)
            );
        }
        notes
    };
    let mut notes = notes;

    // 4. 收集被选扩展对依赖线的约束。
    let mut constraints: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
    for ext_name in selected_exts {
        let Some(meta) = ext_meta(metas, ext_name) else {
            bail!("未知扩展 ext:{ext_name}");
        };
        // builtin 扩展（如 ext:openssl）没有自己的版本线，
        // 用 PHP_LINES 约束同名的依赖库。
        if !meta.php_lines.is_empty() {
            let Some(allowed) = meta.php_lines.get(php_series) else {
                bail!("ext:{ext_name} 没有声明 PHP {php_series} 的兼容线");
            };
            if !catalog.components.contains_key(&meta.name) {
                bail!(
                    "ext:{ext_name} 声明了 PHP_LINES，但 catalog 没有同名组件 {}",
                    meta.name
                );
            }
            constraints
                .entry(meta.name.clone())
                .or_default()
                .push(allowed.clone());
        }
        if meta.lines.is_empty() {
            continue;
        }
        let Some(choice) = choices.get(ext_name) else {
            bail!(
                "请为扩展 {ext_name} 指定版本线，例如 --ext {ext_name}=version:{}",
                meta.lines.keys().next().cloned().unwrap_or_default()
            );
        };
        let Some(line_meta) = meta.lines.get(&choice.line) else {
            bail!("ext:{ext_name} 的 recipe 没有声明版本线 {}", choice.line);
        };
        for (dep, allowed) in &line_meta.deps {
            if !catalog.components.contains_key(dep) {
                bail!(
                    "ext:{ext_name} 的版本线 {} 依赖 {dep}，但 catalog 没有这个组件",
                    choice.line
                );
            }
            constraints
                .entry(dep.clone())
                .or_default()
                .push(allowed.clone());
        }
    }

    // 5. 对每个被约束的依赖求交集，按硬 pin / 软默认 / 唯一候选处理。
    for (dep, consumer_sets) in constraints {
        let dep_lines = catalog.lines(&dep).expect("第 4 步刚刚检查过");
        let mut allowed: BTreeSet<String> = if let Some(meta) = component_meta(metas, &dep) {
            if meta.lines.is_empty() {
                dep_lines.keys().cloned().collect()
            } else {
                meta.lines
                    .iter()
                    .filter(|(line, lm)| dep_lines.contains_key(*line) && php_in(lm, php_series))
                    .map(|(line, _)| line.clone())
                    .collect()
            }
        } else {
            dep_lines.keys().cloned().collect()
        };
        for set in consumer_sets {
            allowed.retain(|line| set.iter().any(|s| s == line));
        }

        let current = choices.get(&dep).cloned();
        match current {
            Some(choice) if allowed.contains(&choice.line) => {}
            Some(choice) if choice.source == Source::Pinned => {
                bail!(
                    "{dep} {} 与当前 PHP/扩展选择冲突（可用：{}）",
                    choice.line,
                    list(&allowed)
                );
            }
            Some(choice) if choice.source == Source::Bundle && allowed.len() == 1 => {
                let line = allowed.iter().next().expect("刚刚判断只有一个").clone();
                notes.push(format!(
                    "{dep} 的默认线 {} 与当前选择冲突，自动改用 {line}",
                    choice.line
                ));
                choices.insert(
                    dep.clone(),
                    Choice {
                        line,
                        source: Source::Auto,
                    },
                );
            }
            Some(choice) => {
                bail!(
                    "{dep} {} 与当前 PHP/扩展选择冲突（可用：{}）",
                    choice.line,
                    list(&allowed)
                );
            }
            None => {
                if allowed.len() == 1 {
                    let line = allowed.iter().next().expect("刚刚判断只有一个").clone();
                    notes.push(format!("{dep} 自动使用唯一可用线 {line}"));
                    choices.insert(
                        dep.clone(),
                        Choice {
                            line,
                            source: Source::Auto,
                        },
                    );
                } else if allowed.is_empty() {
                    bail!("没有 {dep} 版本线同时满足 PHP {php_series} 和已选扩展");
                } else {
                    bail!("请用 --dep {dep}=<线> 指定版本（可用：{}）", list(&allowed));
                }
            }
        }
    }

    // 6. 生成最终绑定。
    let mut bindings = Bindings {
        notes,
        ..Bindings::default()
    };
    for (name, choice) in choices {
        let artifact = catalog
            .get(&name, &choice.line)
            .expect("第 2 步刚刚校验过")
            .clone();
        bindings.lines.insert(name.clone(), choice.line);
        bindings.artifacts.insert(name, artifact);
    }
    Ok(bindings)
}

/// 生成注入给 recipe 的公共 ctx；每个 recipe 还会再补一个 self。
pub fn context(php_series: &str, php_version: &str, bindings: &Bindings) -> serde_json::Value {
    let mut deps = serde_json::Map::new();
    for (name, artifact) in &bindings.artifacts {
        deps.insert(
            name.clone(),
            artifact_json(
                bindings.lines.get(name).map(String::as_str).unwrap_or(""),
                artifact,
            ),
        );
    }
    serde_json::json!({
        "php_series": php_series,
        "php_version": php_version,
        "bindings": bindings.lines,
        "deps": deps,
    })
}

pub fn artifact_json(line: &str, artifact: &Artifact) -> serde_json::Value {
    serde_json::json!({
        "line": line,
        "version": artifact.version,
        "url": artifact.url,
        "sha256": artifact.sha256,
    })
}

fn component_meta<'a>(metas: &'a [RecipeMeta], name: &str) -> Option<&'a RecipeMeta> {
    metas
        .iter()
        .find(|meta| meta.kind == Kind::Dep && meta.name == name)
        .or_else(|| {
            metas
                .iter()
                .find(|meta| meta.kind == Kind::Ext && meta.name == name)
        })
}

fn ext_meta<'a>(metas: &'a [RecipeMeta], name: &str) -> Option<&'a RecipeMeta> {
    metas
        .iter()
        .find(|meta| meta.kind == Kind::Ext && meta.name == name)
}

fn php_in(line: &crate::recipe::LineMeta, php_series: &str) -> bool {
    line.php.iter().any(|p| p == php_series)
}

fn allowed_lines(catalog: &Catalog, meta: &RecipeMeta, php_series: &str) -> BTreeSet<String> {
    let lines = catalog.lines(&meta.name).cloned().unwrap_or_default();
    meta.lines
        .iter()
        .filter(|(line, lm)| lines.contains_key(*line) && php_in(lm, php_series))
        .map(|(line, _)| line.clone())
        .collect()
}

fn list(lines: &BTreeSet<String>) -> String {
    if lines.is_empty() {
        "无".to_owned()
    } else {
        lines.iter().cloned().collect::<Vec<_>>().join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn setup() -> (Catalog, Bundle, Vec<RecipeMeta>) {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        let metas = crate::recipe::load_meta(&dir, &catalog).unwrap();
        let bundle = Bundle::load(&dir.join("bundles/php8.5.toml")).unwrap();
        (catalog, bundle, metas)
    }

    #[test]
    fn builtin_extension_line_belongs_in_deps_table() {
        let (catalog, mut bundle, metas) = setup();
        bundle
            .ext_lines
            .insert("openssl".to_owned(), "3.5".to_owned());
        let err = resolve(&catalog, &bundle, &metas, "8.5", &[], &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("[deps]"), "{err}");
    }

    #[test]
    fn dependency_line_must_not_name_an_extension() {
        let (catalog, mut bundle, metas) = setup();
        bundle
            .dep_lines
            .insert("swoole".to_owned(), "6.2".to_owned());
        let err = resolve(&catalog, &bundle, &metas, "8.5", &[], &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("是扩展"), "{err}");
    }
}
