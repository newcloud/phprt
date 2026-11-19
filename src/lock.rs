//! `--lock` 的解析输入：从 state 生成 resolver 用的版本 pin。
//!
//! 关键约束：pin 只在“当前待解析方案”里生成。state 里已经被 remove 的组件
//! 和它们的历史依赖不参与当前解析，所以调用方必须先算当前方案的节点闭包，
//! 再和 state 求交，不能直接把整份 state 铺成 pin。
//!
//! 数据流：
//!
//! ```text
//! CLI / current scheme -> desired components -> 当前依赖闭包
//!   -> 与 state 求交 -> EffectivePins -> bind::resolve -> plan
//! ```

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};

use crate::bind::Bindings;
use crate::catalog::Catalog;
use crate::state::ProjectState;

/// CLI 对一个名字给出的新选择：线或精确补丁。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliPin {
    /// `--dep zlib=1.3` / `version:1.3`：匹配 catalog 的版本线。
    Line(String),
    /// `--dep openssl=3.5.9`：精确补丁，所属线由 catalog 反查。
    Version { line: String, version: String },
}

impl CliPin {
    pub fn line(&self) -> &str {
        match self {
            CliPin::Line(line) => line,
            CliPin::Version { line, .. } => line,
        }
    }
}

/// resolver 的锁定输入。`lines` 和 `versions` 都只包含当前方案里的名字。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectivePins {
    pub lines: BTreeMap<String, String>,
    pub versions: BTreeMap<String, String>,
}

/// 把 CLI 值规范成线或补丁。先按线精确匹配，再按线里的补丁版本反查所属线。
pub fn cli_pin(catalog: &Catalog, name: &str, value: &str) -> Result<CliPin> {
    let lines = catalog
        .lines(name)
        .ok_or_else(|| anyhow::anyhow!("「{name}」没有可选版本（catalog 里没有这个组件）"))?;
    if lines.contains_key(value) {
        return Ok(CliPin::Line(value.to_owned()));
    }
    for (line, artifacts) in lines {
        if artifacts.iter().any(|artifact| artifact.version == value) {
            return Ok(CliPin::Version {
                line: line.clone(),
                version: value.to_owned(),
            });
        }
    }
    let labels: Vec<String> = lines
        .iter()
        .map(|(line, artifacts)| {
            let versions: Vec<&str> = artifacts
                .iter()
                .map(|artifact| artifact.version.as_str())
                .collect();
            format!("{line}（{}）", versions.join("/"))
        })
        .collect();
    bail!(
        "{name} 的版本 {value} 不在 catalog（可用：{}）",
        labels.join(", ")
    )
}

/// 生成 resolver 输入。
///
/// `desired` 是当前方案的组件 + 当前依赖闭包；只有这里面的名字才会被 pin。
/// `bundled` 里的组件没有独立版本，它的源码跟随产品补丁，不参与 pin；
/// 同名依赖（如 dep openssl）仍按 `state.deps` 参与。
pub fn effective_pins(
    state: &ProjectState,
    desired: &BTreeSet<String>,
    bundled: &BTreeSet<String>,
    cli: &BTreeMap<String, CliPin>,
    lock: bool,
) -> EffectivePins {
    let mut pins = EffectivePins::default();
    for name in desired {
        if let Some(component) = state.components.get(name)
            && !bundled.contains(name)
        {
            if !component.line.is_empty() {
                pins.lines.insert(name.clone(), component.line.clone());
            }
            if lock && !component.version.is_empty() {
                pins.versions
                    .insert(name.clone(), component.version.clone());
            }
        }
        if let Some(dep) = state.deps.get(name) {
            if !dep.line.is_empty() {
                pins.lines.insert(name.clone(), dep.line.clone());
            }
            if lock && !dep.version.is_empty() {
                pins.versions.insert(name.clone(), dep.version.clone());
            }
        }
    }
    // 用户显式钉过的线仍然有效；它和 state 记录一致时不会改变结果。
    for (name, line) in &state.bindings {
        if desired.contains(name) && !line.is_empty() {
            pins.lines.insert(name.clone(), line.clone());
        }
    }
    // CLI 是完整覆盖：选了新线就丢掉同名的旧补丁，不能留下旧 line 的约束。
    for (name, pin) in cli {
        match pin {
            CliPin::Line(line) => {
                pins.lines.insert(name.clone(), line.clone());
                pins.versions.remove(name);
            }
            CliPin::Version { line, version } => {
                pins.lines.insert(name.clone(), line.clone());
                pins.versions.insert(name.clone(), version.clone());
            }
        }
    }
    pins
}

/// `scheme --lock` 导出用的精确补丁表。
///
/// 版本来源按优先级取：state 记录的实际版本 > 本次解析选中的 artifact。
///
/// - bundled 组件的 `version` 是产品补丁，不是它自己的源码版本，跳过；
///   同名的依赖库（如 dep openssl）仍有独立版本，照常导出；
/// - `desired` 是当前闭包，已删除组件留下的历史依赖不导出；
/// - state 里记录的旧补丁必须仍能在 catalog 里找到，否则直接报错，不静默回落；
/// - 闭包内没有 state 记录的节点用 `resolved` 补齐，不能整条 pin 都漏掉。
pub fn exact_versions(
    catalog: &Catalog,
    state: &ProjectState,
    product: &str,
    bundled: &BTreeSet<String>,
    is_dep: &dyn Fn(&str) -> bool,
    desired: &BTreeSet<String>,
    resolved: &Bindings,
) -> Result<BTreeMap<String, String>> {
    // 只有“有独立源码版本”的节点才需要 pin（line, version）。
    let locked = |name: &str| name != product && (!bundled.contains(name) || is_dep(name));

    let mut picked: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (name, component) in &state.components {
        if bundled.contains(name) || component.version.is_empty() || !desired.contains(name) {
            continue;
        }
        picked.insert(
            name.clone(),
            (component.line.clone(), component.version.clone()),
        );
    }
    for (name, dep) in &state.deps {
        if dep.version.is_empty() || !desired.contains(name) || !locked(name) {
            continue;
        }
        picked.insert(name.clone(), (dep.line.clone(), dep.version.clone()));
    }
    for name in desired {
        if picked.contains_key(name) || !locked(name) {
            continue;
        }
        let Some(artifact) = resolved.artifacts.get(name) else {
            bail!("闭包里的 {name} 没有解析出 artifact，无法生成可复现的 lock");
        };
        let line = resolved.lines.get(name).cloned().unwrap_or_default();
        picked.insert(name.clone(), (line, artifact.version.clone()));
    }

    let mut exact: BTreeMap<String, String> = BTreeMap::new();
    for (name, (line, version)) in picked {
        if line.is_empty() || catalog.artifact_for(&name, &line, &version).is_none() {
            let versions: Vec<&str> = catalog
                .lines(&name)
                .and_then(|lines| lines.get(&line))
                .map(|artifacts| {
                    artifacts
                        .iter()
                        .map(|artifact| artifact.version.as_str())
                        .collect()
                })
                .unwrap_or_default();
            bail!(
                "lock 里的 {name} {version} 不在 catalog（线 {line} 可用：{}）；\
                 用 phprt update {}@{} 升级，或在 catalog 里保留旧补丁",
                versions.join(", "),
                state.product,
                state.series
            );
        }
        exact.insert(name, version);
    }
    Ok(exact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{ComponentState, DepState};
    use std::collections::BTreeMap;

    fn state() -> ProjectState {
        let mut state = ProjectState {
            version: 3,
            active: true,
            product: "php".into(),
            series: "8.5".into(),
            installed_version: "8.5.12".into(),
            ..ProjectState::default()
        };
        state.components.insert(
            "swoole".into(),
            ComponentState {
                line: "6.2".into(),
                version: "6.2.5".into(),
                mode: "shared".into(),
                ..Default::default()
            },
        );
        state.components.insert(
            "redis".into(),
            ComponentState {
                line: "6.3".into(),
                version: "6.3.1".into(),
                mode: "shared".into(),
                ..Default::default()
            },
        );
        state.deps.insert(
            "openssl".into(),
            DepState {
                line: "3.5".into(),
                version: "3.5.9".into(),
                ..Default::default()
            },
        );
        state.deps.insert(
            "zlib".into(),
            DepState {
                line: "1.3".into(),
                version: "1.3.2".into(),
                ..Default::default()
            },
        );
        state.deps.insert(
            "hiredis".into(),
            DepState {
                line: "1.2".into(),
                version: "1.2.0".into(),
                ..Default::default()
            },
        );
        state
    }

    fn names(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pins_only_cover_the_current_scheme_closure() {
        // 当前方案只剩 php + swoole；redis 和它带来的 hiredis 只是历史记录。
        let pins = effective_pins(
            &state(),
            &names(&["php", "swoole", "openssl"]),
            &names(&["openssl"]),
            &BTreeMap::new(),
            true,
        );
        assert_eq!(pins.lines["swoole"], "6.2");
        assert_eq!(pins.versions["swoole"], "6.2.5");
        assert_eq!(pins.versions["openssl"], "3.5.9");
        assert!(!pins.lines.contains_key("redis"));
        assert!(!pins.versions.contains_key("redis"));
        assert!(!pins.lines.contains_key("hiredis"));
        assert!(!pins.versions.contains_key("hiredis"));
    }

    #[test]
    fn without_lock_only_lines_are_pinned() {
        let pins = effective_pins(
            &state(),
            &names(&["swoole", "openssl", "zlib"]),
            &names(&["openssl"]),
            &BTreeMap::new(),
            false,
        );
        assert_eq!(pins.lines["swoole"], "6.2");
        assert_eq!(pins.lines["zlib"], "1.3");
        assert!(pins.versions.is_empty());
    }

    #[test]
    fn bundled_component_version_is_not_a_source_patch() {
        let mut state = state();
        state.components.insert(
            "openssl".into(),
            ComponentState {
                line: "3.5".into(),
                version: "8.5.12".into(),
                mode: "static".into(),
                ..Default::default()
            },
        );
        let pins = effective_pins(
            &state,
            &names(&["openssl"]),
            &names(&["openssl"]),
            &BTreeMap::new(),
            true,
        );
        // 组件 openssl 跟随产品补丁；同名依赖库的 3.5.9 仍然要锁。
        assert_eq!(pins.versions["openssl"], "3.5.9");
    }

    #[test]
    fn stale_bindings_of_removed_components_are_not_pinned() {
        let mut state = state();
        state.bindings.insert("redis".into(), "6.3".into());
        state.bindings.insert("openssl".into(), "3.5".into());
        let pins = effective_pins(
            &state,
            &names(&["php", "swoole", "openssl"]),
            &names(&["openssl"]),
            &BTreeMap::new(),
            true,
        );
        assert!(!pins.lines.contains_key("redis"));
        assert_eq!(pins.lines["openssl"], "3.5");
    }

    #[test]
    fn cli_line_replaces_the_whole_binding() {
        let cli = BTreeMap::from([("openssl".to_owned(), CliPin::Line("4.0".to_owned()))]);
        let pins = effective_pins(
            &state(),
            &names(&["openssl"]),
            &names(&["openssl"]),
            &cli,
            true,
        );
        assert_eq!(pins.lines["openssl"], "4.0");
        assert!(!pins.versions.contains_key("openssl"));
    }

    #[test]
    fn cli_patch_replaces_the_whole_binding() {
        let cli = BTreeMap::from([(
            "openssl".to_owned(),
            CliPin::Version {
                line: "4.0".to_owned(),
                version: "4.0.0".to_owned(),
            },
        )]);
        let pins = effective_pins(
            &state(),
            &names(&["openssl"]),
            &names(&["openssl"]),
            &cli,
            true,
        );
        assert_eq!(pins.lines["openssl"], "4.0");
        assert_eq!(pins.versions["openssl"], "4.0.0");
    }

    #[test]
    fn updated_state_is_the_next_lock_baseline() {
        let mut state = state();
        // 模拟一次成功的 update：线不变，版本跟着 catalog 前进。
        state.deps.get_mut("openssl").unwrap().version = "3.5.10".into();
        let pins = effective_pins(
            &state,
            &names(&["openssl"]),
            &names(&["openssl"]),
            &BTreeMap::new(),
            true,
        );
        assert_eq!(pins.versions["openssl"], "3.5.10");
    }

    #[test]
    fn exact_versions_only_cover_the_current_closure() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        let mut state = state();
        // 已删除组件留下的历史依赖，而且这个补丁已经不在 catalog。
        state.deps.insert(
            "libxml2".into(),
            DepState {
                line: "2.15".into(),
                version: "2.15.0".into(),
                ..Default::default()
            },
        );
        let bundled = names(&["openssl"]);
        let is_dep = |name: &str| name == "openssl" || name == "zlib";
        let resolved = Bindings::default();
        let exact = exact_versions(
            &catalog,
            &state,
            "php",
            &bundled,
            &is_dep,
            &names(&["php", "swoole", "openssl"]),
            &resolved,
        )
        .unwrap();
        assert_eq!(exact["swoole"], "6.2.5");
        assert_eq!(exact["openssl"], "3.5.9");
        assert!(!exact.contains_key("php"));
        assert!(!exact.contains_key("libxml2"));

        // 真把它算进闭包时才报错，而且不改成 catalog 的当前补丁。
        let err = exact_versions(
            &catalog,
            &state,
            "php",
            &bundled,
            &is_dep,
            &names(&["php", "swoole", "openssl", "libxml2"]),
            &resolved,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("libxml2 2.15.0"), "{err}");
    }

    #[test]
    fn exact_versions_fill_nodes_without_state_record() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        let mut state = state();
        // state 没有 zlib 的版本记录：不能因此整条 pin 都漏掉。
        state.deps.remove("zlib");
        let mut resolved = Bindings::default();
        let artifact = catalog.get("zlib", "1.3").unwrap().clone();
        resolved.lines.insert("zlib".into(), "1.3".into());
        resolved.artifacts.insert("zlib".into(), artifact.clone());

        let exact = exact_versions(
            &catalog,
            &state,
            "php",
            &names(&["openssl"]),
            &|name: &str| name == "openssl" || name == "zlib",
            &names(&["php", "swoole", "openssl", "zlib"]),
            &resolved,
        )
        .unwrap();
        assert_eq!(exact["zlib"], artifact.version.as_str());
        // bundled 组件本身没有源码版本，同名的依赖库才有。
        assert!(!exact.contains_key("php"));
    }

    #[test]
    fn cli_value_matches_line_before_patch() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        assert_eq!(
            cli_pin(&catalog, "openssl", "4.0").unwrap(),
            CliPin::Line("4.0".into())
        );
        assert_eq!(
            cli_pin(&catalog, "openssl", "3.5.9").unwrap(),
            CliPin::Version {
                line: "3.5".into(),
                version: "3.5.9".into()
            }
        );
        let err = cli_pin(&catalog, "openssl", "3.5.99")
            .unwrap_err()
            .to_string();
        assert!(err.contains("3.5.9"), "{err}");
    }
}
