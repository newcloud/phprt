use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::recipe::{ComponentSpec, Registry, Selection};

/// `recipes/bundles/<产品>/<系列>.toml`：某个产品系列的默认策略。
///
/// - 顶层：有独立版本线的组件 -> 默认线；只存线，不存补丁号。
/// - `[deps]`：依赖库 -> 默认线。内置组件没有自己的版本线，它的同名库写在这里。
/// - `[component_presets]`：组件被选中时的默认 feature / 依赖功能，不决定是否默认启用。
/// - `[default_components]`：默认启用的组件；可用 `ref` 引用 `[component_presets]`。
#[derive(Debug, Clone)]
pub struct Bundle {
    pub product: String,
    pub component_lines: BTreeMap<String, String>,
    pub dep_lines: BTreeMap<String, String>,
    pub component_presets: BTreeMap<String, ComponentDefault>,
    pub default_components: BTreeMap<String, ComponentDefault>,
}

/// 一个组件的默认配置：挂载方式可选，feature 和依赖功能都是集合。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentDefault {
    /// 只用于 `[default_components]`：引用 `component_presets.<名字>`。
    #[serde(default, rename = "ref")]
    pub reference: String,
    /// 省略时按 source 推导。
    #[serde(default)]
    pub mode: Option<String>,
    /// 组件自己的 feature，如 `["argon2"]`。
    #[serde(default)]
    pub features: Vec<String>,
    /// 依赖名 -> 转给它的 feature，如 `{ libcurl = ["ssl"] }`。
    #[serde(default)]
    pub deps: BTreeMap<String, Vec<String>>,
}

impl Bundle {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("读不到 {}", path.display()))?;
        let raw: toml::Table = toml::from_str(&text)
            .with_context(|| format!("{} 不是合法的 bundle", path.display()))?;

        let mut component_lines = BTreeMap::new();
        let mut dep_lines = BTreeMap::new();
        let mut component_presets = BTreeMap::new();
        let mut product = None;
        let mut default_components = BTreeMap::new();
        for (key, value) in raw {
            if key == "product" {
                match value {
                    toml::Value::String(text) => product = Some(text),
                    _ => bail!("{} 的 product 必须是字符串", path.display()),
                }
            } else if key == "deps" {
                dep_lines = value.try_into().with_context(|| {
                    format!(
                        "{} 的 [deps] 必须是「依赖库名 -> 版本线字符串」",
                        path.display()
                    )
                })?;
            } else if key == "component_presets" {
                component_presets = value
                    .try_into()
                    .with_context(|| format!("{} 的 [component_presets] 不合法", path.display()))?;
            } else if key == "default_components" {
                default_components = value.try_into().with_context(|| {
                    format!("{} 的 [default_components] 不合法", path.display())
                })?;
            } else {
                match value {
                    toml::Value::String(text) => {
                        component_lines.insert(key, text);
                    }
                    _ => bail!(
                        "{} 的 {key} 必须是版本线字符串：组件默认线写顶层，依赖库默认线写在 [deps]，\
                         组件预设写在 [component_presets]，默认组件写在 [default_components]",
                        path.display()
                    ),
                }
            }
        }

        let product = product.ok_or_else(|| anyhow::anyhow!("{} 缺少 product", path.display()))?;
        let bundle = Self {
            product,
            component_lines,
            dep_lines,
            component_presets,
            default_components,
        };
        bundle.validate_refs(path)?;
        Ok(bundle)
    }

    fn validate_refs(&self, path: &Path) -> Result<()> {
        for (name, def) in &self.component_presets {
            if name.is_empty() {
                bail!("{}: [component_presets] 里有空名字", path.display());
            }
            if !def.reference.is_empty() {
                bail!("{}: component_presets.{name} 不能写 ref", path.display());
            }
        }

        for (name, def) in &self.default_components {
            if def.reference.is_empty() {
                continue;
            }
            let preset_name = match preset_name(&def.reference) {
                Ok(name) => name,
                Err(err) => bail!(
                    "{}: default_components.{name} 的 ref 不合法：{err}",
                    path.display()
                ),
            };
            if preset_name != name {
                bail!(
                    "{}: default_components.{name} 的 ref 必须指向 component_presets.{name}",
                    path.display()
                );
            }
            if !self.component_presets.contains_key(preset_name) {
                bail!(
                    "{}: default_components.{name} 的 ref 指向不存在的 component_presets.{preset_name}",
                    path.display()
                );
            }
            if !def.features.is_empty() || !def.deps.is_empty() {
                bail!(
                    "{}: default_components.{name} 使用 ref 时不能再写 features/deps；\
                     需要不同配置时另建 preset",
                    path.display()
                );
            }
        }
        Ok(())
    }

    fn resolve_default(&self, name: &str) -> Result<ComponentDefault> {
        let def = self
            .default_components
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("[default_components] 里没有 {name}"))?;
        if def.reference.is_empty() {
            return Ok(def.clone());
        }

        let preset_name = preset_name(&def.reference)?;
        let preset = self.component_presets.get(preset_name).ok_or_else(|| {
            anyhow::anyhow!("default_components.{name} 的 ref 指向不存在的 preset")
        })?;
        let mut merged = preset.clone();
        if let Some(mode) = &def.mode {
            merged.mode = Some(mode.clone());
        }
        Ok(merged)
    }

    /// `[default_components]` 解析 ref 后实际使用的 feature。
    pub fn default_features(&self, name: &str) -> Option<&[String]> {
        let def = self.default_components.get(name)?;
        if def.reference.is_empty() {
            Some(&def.features)
        } else {
            let preset_name = preset_name(&def.reference).ok()?;
            self.component_presets
                .get(preset_name)
                .map(|preset| preset.features.as_slice())
        }
    }

    /// `[component_presets]` 里的 feature；不解析 ref。
    pub fn preset_features(&self, name: &str) -> Option<&[String]> {
        self.component_presets
            .get(name)
            .map(|preset| preset.features.as_slice())
    }

    pub fn check_product(&self, product: &str) -> Result<()> {
        if self.product != product {
            bail!(
                "bundle 的 product={} 与当前产品 {product} 不一致",
                self.product
            );
        }
        Ok(())
    }
}

fn preset_name(reference: &str) -> Result<&str> {
    let Some(name) = reference.strip_prefix("component_presets.") else {
        bail!("ref 必须是 \"component_presets.<名字>\"，例如 \"component_presets.gd\"");
    };
    if name.is_empty() || name.contains('.') {
        bail!("ref 必须是 \"component_presets.<名字>\"，例如 \"component_presets.gd\"");
    }
    Ok(name)
}

/// 把 bundle 的 `[default_components]` 转成与 `--component` 一致的 Selection。
///
/// 当前产品 / 依赖线不满足的 feature 只提示并丢弃；写错依赖或 feature 名直接报错。
pub fn selection(reg: &Registry, bundle: &Bundle, series: &str) -> Result<Selection> {
    let mut sel = Selection::default();
    for name in bundle.default_components.keys() {
        let def = bundle.resolve_default(name)?;
        let spec = build_spec(reg, name, &def, series, "[default_components]")?;
        sel.per_component.insert(name.clone(), spec);
    }
    Ok(sel)
}

/// 把 bundle 的 `[component_presets]` 转成与 `--component` 一致的 Selection。
///
/// 只提供预设，不会把组件加入默认方案。
pub fn presets(reg: &Registry, bundle: &Bundle, series: &str) -> Result<Selection> {
    let mut sel = Selection::default();
    for (name, def) in &bundle.component_presets {
        let spec = build_spec(reg, name, def, series, "[component_presets]")?;
        sel.per_component.insert(name.clone(), spec);
    }
    Ok(sel)
}

fn build_spec(
    reg: &Registry,
    name: &str,
    def: &ComponentDefault,
    series: &str,
    table: &str,
) -> Result<ComponentSpec> {
    let component = reg.component(name)?;
    let mut spec = ComponentSpec::default();

    if let Some(mode) = &def.mode {
        if !component.modes.iter().any(|m| m == mode) {
            bail!(
                "bundle {series}.toml 的 {table} 里 component:{name} 不支持 mode={mode}\
                 （支持：{}）",
                component.modes.join(",")
            );
        }
        reg.product_mode(component, mode)?;
        spec.mode = Some(mode.clone());
    }

    let mut features = BTreeSet::new();
    for f in &def.features {
        if !component.features.contains_key(f) {
            eprintln!(
                "注意: bundle {series}.toml 的 {table} 里 component:{name} 没有 feature {f}，已丢弃"
            );
            continue;
        }
        if let Err(err) = crate::recipe::feature_available(reg, component, f) {
            eprintln!("注意: bundle {series}.toml 的 {table} 丢弃 {err}");
            continue;
        }
        features.insert(f.clone());
    }
    spec.features = Some(features);

    for (dep, feats) in &def.deps {
        if !component.dep_configure.contains_key(dep) {
            bail!("bundle {series}.toml 的 {table} 里 component:{name} 没有依赖集成 dep:{dep}");
        }
        let target = reg
            .get(&format!("dep:{dep}"))
            .ok_or_else(|| anyhow::anyhow!("component:{name} 的 dep:{dep} 指向不存在的依赖"))?;
        let mut available = Vec::new();
        for f in feats {
            if !target.features.contains_key(f) {
                bail!("bundle {series}.toml 的 {table} 里 dep:{dep} 没有 feature {f}");
            }
            if let Err(err) = crate::recipe::feature_available(reg, target, f) {
                eprintln!("注意: bundle {series}.toml 的 {table} 丢弃 {err}");
                continue;
            }
            available.push(f.clone());
        }
        if !available.is_empty() {
            spec.deps.insert(dep.clone(), available);
        }
    }

    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes")
    }

    #[test]
    fn default_components_is_a_table_of_features_and_deps() {
        let bundle = Bundle::load(&dir().join("bundles/php/8.5.toml")).unwrap();
        let openssl = &bundle.default_components["openssl"];
        assert_eq!(openssl.reference, "component_presets.openssl");
        assert!(openssl.features.is_empty());
        assert!(openssl.deps.is_empty());
        assert_eq!(openssl.mode, None);

        let curl = &bundle.default_components["curl"];
        assert_eq!(curl.reference, "component_presets.curl");
        assert!(curl.features.is_empty());
        assert!(curl.deps.is_empty());
        assert_eq!(bundle.component_lines["swoole"], "6.2");
        assert_eq!(bundle.dep_lines["openssl"], "3.5");
        assert!(!bundle.component_lines.contains_key("openssl"));

        assert_eq!(
            bundle.component_presets["openssl"].features,
            vec!["argon2".to_owned()]
        );
        assert_eq!(
            bundle.component_presets["curl"].deps["libcurl"],
            vec!["ssl".to_owned()]
        );
        let gd = &bundle.component_presets["gd"];
        assert_eq!(
            gd.features,
            vec!["jpeg".to_owned(), "webp".to_owned(), "freetype".to_owned()]
        );
        assert!(gd.reference.is_empty());
        assert!(!bundle.default_components.contains_key("gd"));
    }

    #[test]
    fn other_series_have_no_default_components() {
        let bundle = Bundle::load(&dir().join("bundles/php/7.4.toml")).unwrap();
        assert!(bundle.default_components.is_empty());
        assert!(bundle.component_presets.is_empty());
    }

    #[test]
    fn ref_uses_preset_and_allows_mode_override() {
        let path = temp_path("ref");
        std::fs::write(
            &path,
            r#"
product = "php"
[component_presets]
gd = { features = ["jpeg"] }
[default_components]
gd = { ref = "component_presets.gd", mode = "shared" }
"#,
        )
        .unwrap();
        let bundle = Bundle::load(&path).unwrap();
        let default = bundle.resolve_default("gd").unwrap();
        assert_eq!(default.features, vec!["jpeg".to_owned()]);
        assert_eq!(default.mode.as_deref(), Some("shared"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_without_ref_ignores_same_named_preset() {
        let path = temp_path("local");
        std::fs::write(
            &path,
            r#"
product = "php"
[component_presets]
gd = { features = ["jpeg"] }
[default_components]
gd = { features = ["webp"] }
"#,
        )
        .unwrap();
        let bundle = Bundle::load(&path).unwrap();
        let default = bundle.resolve_default("gd").unwrap();
        assert_eq!(default.features, vec!["webp".to_owned()]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ref_must_match_name_and_not_mix_local_features() {
        for (name, body, expected) in [
            (
                "mismatch",
                r#"product = "php"
[component_presets]
gd = { features = ["jpeg"] }
[default_components]
curl = { ref = "component_presets.gd" }
"#,
                "ref 必须指向 component_presets.curl",
            ),
            (
                "mixed",
                r#"product = "php"
[component_presets]
gd = { features = ["jpeg"] }
[default_components]
gd = { ref = "component_presets.gd", features = ["webp"] }
"#,
                "使用 ref 时不能再写 features/deps",
            ),
            (
                "missing",
                r#"product = "php"
[default_components]
gd = { ref = "component_presets.gd" }
"#,
                "指向不存在的 component_presets.gd",
            ),
            (
                "badpath",
                r#"product = "php"
[default_components]
gd = { ref = "gd" }
"#,
                "ref 必须是 \"component_presets.<名字>\"",
            ),
        ] {
            let path = temp_path(name);
            std::fs::write(&path, body).unwrap();
            let err = Bundle::load(&path).unwrap_err().to_string();
            assert!(err.contains(expected), "{name}: {err}");
            let _ = std::fs::remove_file(&path);
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("phprt-bundle-{name}-{}", std::process::id()))
    }

    #[test]
    fn line_values_must_not_be_tables() {
        let path = temp_path("line");
        std::fs::write(
            &path,
            "product = \"php\"\nopenssl = { features = [\"argon2\"] }\n",
        )
        .unwrap();
        let err = Bundle::load(&path).unwrap_err().to_string();
        assert!(err.contains("必须是版本线字符串"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn deps_values_must_be_lines() {
        let path = temp_path("deps");
        std::fs::write(
            &path,
            "product = \"php\"\n[deps]\nopenssl = { line = \"3.5\" }\n",
        )
        .unwrap();
        let err = Bundle::load(&path).unwrap_err().to_string();
        assert!(err.contains("[deps]"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unknown_default_component_field_is_an_error() {
        let path = temp_path("unknown");
        std::fs::write(
            &path,
            "product = \"php\"\n[default_components]\ncurl = { dep = { libcurl = [\"ssl\"] } }\n",
        )
        .unwrap();
        let err = Bundle::load(&path).unwrap_err().to_string();
        assert!(err.contains("[default_components]"), "{err}");
        let _ = std::fs::remove_file(&path);
    }
}
