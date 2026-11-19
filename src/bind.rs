use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};

use crate::bundle::Bundle;
use crate::catalog::{Artifact, Catalog};
use crate::lock::EffectivePins;
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
    product: &str,
    series: &str,
    selected_components: &[String],
    pins: &EffectivePins,
) -> Result<Bindings> {
    // 1. 显式指定的线必须存在。
    for (name, line) in &pins.lines {
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

    for (name, line) in &bundle.component_lines {
        let Some(meta) = component_meta(metas, name) else {
            bail!("bundle 里的 {name} 不是组件");
        };
        if meta.lines.is_empty() {
            if metas
                .iter()
                .any(|meta| meta.kind == Kind::Dep && meta.name == *name)
            {
                bail!(
                    "bundle 里的 {name} 是内置组件，没有自己的版本线；\
                     依赖库 {name} 的默认线请写进 [deps]"
                );
            }
            bail!("bundle 里的 {name} 是内置组件，没有自己的版本线");
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
            if component_meta(metas, name).is_some() {
                bail!("bundle [deps] 里的 {name} 是组件，默认线请写在顶层");
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
    for name in bundle.component_lines.keys() {
        if bundle.dep_lines.contains_key(name) {
            bail!("bundle 里的 {name} 同时写在了顶层和 [deps]");
        }
    }
    let mut defaults = bundle.component_lines.clone();
    defaults.extend(bundle.dep_lines.clone());

    let product_names: BTreeSet<&str> = metas
        .iter()
        .filter(|meta| meta.kind == Kind::Product)
        .map(|meta| meta.name.as_str())
        .collect();

    // 2. 初始选择：pins > bundle > 唯一的线。
    let mut choices: BTreeMap<String, Choice> = BTreeMap::new();
    for name in catalog
        .components
        .keys()
        .filter(|name| !product_names.contains(name.as_str()))
    {
        let lines = catalog.lines(name).expect("catalog 刚刚校验过");
        let choice = if let Some(line) = pins.lines.get(name) {
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

    // 3. 每一条已绑定的线都要满足它自己的产品兼容表。
    // skipped 记录「只有一条线但不支持当前系列」的组件，供第 4 步给出准确报错。
    let mut skipped: BTreeMap<String, String> = BTreeMap::new();
    let mut notes = Vec::new();
    {
        for (name, choice) in choices.clone() {
            let Some(meta) = component_meta_of(metas, product, &name) else {
                continue;
            };
            if meta.lines.is_empty() {
                continue;
            }
            let allowed = allowed_lines(catalog, meta, series);
            if allowed.contains(&choice.line) {
                continue;
            }
            if choice.source == Source::Pinned {
                bail!(
                    "{name} {} 不支持产品 {series}（可用：{}）",
                    choice.line,
                    list(&allowed)
                );
            }
            if choice.source == Source::Single {
                // catalog 里只有一条线的组件会被自动绑定；如果这条线不支持当前
                // 系列，就当作该系列没有这个组件，不加载它的 recipe。
                skipped.insert(name.clone(), choice.line.clone());
                choices.remove(&name);
                continue;
            }
            if choice.source == Source::Bundle {
                bail!(
                    "bundle 里 {name} = {} 不支持产品 {series}（可用：{}）；\
                     改正 bundles/{product}/{series}.toml",
                    choice.line,
                    list(&allowed)
                );
            }
            bail!(
                "{name} {} 不支持产品 {series}（可用：{}）",
                choice.line,
                list(&allowed)
            );
        }
    }

    // 4. 收集被选组件和产品本身对依赖线的约束。内置组件一定会加载（is_bundled），
    //    即便不在本次方案里，它的 PRODUCT_LINES / PRODUCT_DEP_LINES 也要参与约束，
    //    否则多线依赖库（如 libcurl）会留在未绑定状态。
    let product_meta = metas
        .iter()
        .find(|meta| {
            meta.kind == Kind::Product
                && meta.name == product
                && meta.series.as_deref() == Some(series)
        })
        .ok_or_else(|| anyhow::anyhow!("没有找到产品 {product}@{series} 的 recipe 元数据"))?;
    let bundled = product_meta.bundled.clone();
    let mut considered: Vec<String> = selected_components.to_vec();
    for name in &bundled {
        // 没有 recipe 的内置组件只影响产品 configure，不参与依赖线约束。
        if component_meta_of(metas, product, name).is_some() && !considered.contains(name) {
            considered.push(name.clone());
        }
    }
    let mut constraints: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();

    // 产品自己的版本线可以约束依赖（如 nginx 1.29 允许哪些 OpenSSL / PCRE2）。
    if let Some(line_meta) = product_meta.lines.get(series) {
        for (dep, allowed) in &line_meta.deps {
            if !catalog.components.contains_key(dep) {
                bail!("产品 {product}@{series} 依赖 {dep}，但 catalog 没有这个组件");
            }
            constraints
                .entry(dep.clone())
                .or_default()
                .push(allowed.clone());
        }
    }

    for component_name in &considered {
        let Some(meta) = component_meta_of(metas, product, component_name) else {
            bail!("未知组件 component:{component_name}");
        };
        // 内置组件（如 component:openssl）没有自己的版本线，
        // 用 PRODUCT_LINES 约束同名的依赖库。
        if !meta.product_lines.is_empty() {
            let Some(allowed) = meta.product_lines.get(series) else {
                bail!("component:{component_name} 没有声明产品 {series} 的兼容线");
            };
            if !catalog.components.contains_key(&meta.name) {
                bail!(
                    "component:{component_name} 声明了 PRODUCT_LINES，但 catalog 没有同名组件 {}",
                    meta.name
                );
            }
            constraints
                .entry(meta.name.clone())
                .or_default()
                .push(allowed.clone());
        }
        if !meta.product_dep_lines.is_empty() {
            let Some(deps) = meta.product_dep_lines.get(series) else {
                bail!("component:{component_name} 没有声明产品 {series} 的依赖兼容线");
            };
            for (dep, allowed) in deps {
                if !catalog.components.contains_key(dep) {
                    bail!(
                        "component:{component_name} 的 PRODUCT_DEP_LINES 引用了 catalog 没有的 {dep}"
                    );
                }
                constraints
                    .entry(dep.clone())
                    .or_default()
                    .push(allowed.clone());
            }
        }
        if meta.lines.is_empty() {
            continue;
        }
        if let Some(line) = skipped.get(component_name) {
            bail!("component:{component_name} {line} 不支持产品 {series}，该系列不加载这个组件");
        }
        let Some(choice) = choices.get(component_name) else {
            bail!(
                "请为组件 {component_name} 指定版本线，例如 --component {component_name}=version:{}",
                meta.lines.keys().next().cloned().unwrap_or_default()
            );
        };
        let Some(line_meta) = meta.lines.get(&choice.line) else {
            bail!(
                "component:{component_name} 的 recipe 没有声明版本线 {}",
                choice.line
            );
        };
        for (dep, allowed) in &line_meta.deps {
            if !catalog.components.contains_key(dep) {
                bail!(
                    "component:{component_name} 的版本线 {} 依赖 {dep}，但 catalog 没有这个组件",
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
                    .filter(|(line, lm)| dep_lines.contains_key(*line) && product_in(lm, series))
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
                    "{dep} {} 与当前产品/组件选择冲突（可用：{}）",
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
                    "{dep} {} 与当前产品/组件选择冲突（可用：{}）",
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
                    bail!("没有 {dep} 版本线同时满足产品 {series} 和已选组件");
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
        // `--lock` 给了精确补丁就用它；找不到直接报错，不回落当前补丁。
        let artifact = match pins.versions.get(&name) {
            Some(version) => catalog
                .artifact_for(&name, &choice.line, version)
                .ok_or_else(|| {
                    locked_patch_missing(&name, &choice.line, version, catalog, product, series)
                })?,
            None => catalog
                .get(&name, &choice.line)
                .expect("第 2 步刚刚校验过")
                .clone(),
        };
        bindings.lines.insert(name.clone(), choice.line);
        bindings.artifacts.insert(name, artifact);
    }
    Ok(bindings)
}

fn locked_patch_missing(
    name: &str,
    line: &str,
    version: &str,
    catalog: &Catalog,
    product: &str,
    series: &str,
) -> anyhow::Error {
    let current = catalog.get(name, line);
    let current_version = current
        .map(|artifact| artifact.version.as_str())
        .unwrap_or("无");
    let mut available = vec![current_version.to_owned()];
    if let Some(current) = current {
        available.extend(current.patches.iter().map(|patch| patch.version.clone()));
    }
    anyhow::anyhow!(
        "lock 里的 {name} {version} 不在 catalog（线 {line} 当前 {current_version}，\
         可用：{}）；用 phprt update {product}@{series} 升级，或在 catalog 里保留旧补丁",
        available.join(", ")
    )
}

/// 生成注入给 recipe 的公共 ctx；每个 recipe 还会再补一个 self。
pub fn context(
    product: &str,
    product_series: &str,
    product_version: &str,
    bindings: &Bindings,
) -> serde_json::Value {
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
        "product": product,
        "product_series": product_series,
        "product_version": product_version,
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

/// 某产品下的组件元数据；同名依赖库不参与匹配。
fn component_meta_of<'a>(
    metas: &'a [RecipeMeta],
    product: &str,
    name: &str,
) -> Option<&'a RecipeMeta> {
    metas.iter().find(|meta| {
        meta.kind == Kind::Component
            && meta.name == name
            && meta.product.as_deref() == Some(product)
    })
}

fn component_meta<'a>(metas: &'a [RecipeMeta], name: &str) -> Option<&'a RecipeMeta> {
    metas
        .iter()
        .find(|meta| meta.kind == Kind::Dep && meta.name == name)
        .or_else(|| {
            metas
                .iter()
                .find(|meta| meta.kind == Kind::Component && meta.name == name)
        })
}

fn product_in(line: &crate::recipe::LineMeta, series: &str) -> bool {
    line.product.iter().any(|p| p == series)
}

fn allowed_lines(catalog: &Catalog, meta: &RecipeMeta, series: &str) -> BTreeSet<String> {
    let lines = catalog.lines(&meta.name).cloned().unwrap_or_default();
    meta.lines
        .iter()
        .filter(|(line, lm)| lines.contains_key(*line) && product_in(lm, series))
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
        let bundle = Bundle::load(&dir.join("bundles/php/8.5.toml")).unwrap();
        (catalog, bundle, metas)
    }

    #[test]
    fn bundled_component_line_belongs_in_deps_table() {
        let (catalog, mut bundle, metas) = setup();
        bundle
            .component_lines
            .insert("openssl".to_owned(), "3.5".to_owned());
        let err = resolve(
            &catalog,
            &bundle,
            &metas,
            "php",
            "8.5",
            &[],
            &EffectivePins::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("[deps]"), "{err}");
    }

    #[test]
    fn dependency_line_must_not_name_a_component() {
        let (catalog, mut bundle, metas) = setup();
        bundle
            .dep_lines
            .insert("swoole".to_owned(), "6.2".to_owned());
        let err = resolve(
            &catalog,
            &bundle,
            &metas,
            "php",
            "8.5",
            &[],
            &EffectivePins::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("是组件"), "{err}");
    }

    #[test]
    fn bundle_line_for_the_wrong_series_is_an_error() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        let metas = crate::recipe::load_meta(&dir, &catalog).unwrap();
        let mut bundle = Bundle::load(&dir.join("bundles/php/7.4.toml")).unwrap();
        bundle
            .component_lines
            .insert("xdebug".to_owned(), "3.5".to_owned());
        let err = resolve(
            &catalog,
            &bundle,
            &metas,
            "php",
            "7.4",
            &[],
            &EffectivePins::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("bundle 里 xdebug = 3.5 不支持产品 7.4"),
            "{err}"
        );
        assert!(err.contains("3.1"), "{err}");
    }

    #[test]
    fn single_line_component_not_for_this_series_is_skipped() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        let metas = crate::recipe::load_meta(&dir, &catalog).unwrap();
        let bundle = Bundle::load(&dir.join("bundles/php/7.4.toml")).unwrap();
        let bindings = resolve(
            &catalog,
            &bundle,
            &metas,
            "php",
            "7.4",
            &[],
            &EffectivePins::default(),
        )
        .expect("7.4 加载成功，只是不绑定 protobuf");
        assert!(!bindings.lines.contains_key("protobuf"));

        let err = resolve(
            &catalog,
            &bundle,
            &metas,
            "php",
            "7.4",
            &["protobuf".to_owned()],
            &EffectivePins::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("component:protobuf 5.36 不支持产品 7.4"),
            "{err}"
        );
    }
}
