use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::recipe::{ExtSpec, Mode, Registry, Selection};

/// `recipes/bundles/php<系列>.toml`：某个 PHP 系列的默认策略。
///
/// - 顶层：有独立版本线的扩展 -> 默认线；只存线，不存补丁号。
/// - `[deps]`：依赖库 -> 默认线。内置扩展没有自己的版本线，它的同名库写在这里。
/// - `[ext_presets]`：扩展被选中时的默认 feature / 依赖功能，不决定是否默认启用。
/// - `[default_exts]`：默认启用的扩展；可用 `ref` 引用 `[ext_presets]`。
#[derive(Debug, Clone)]
pub struct Bundle {
    pub php: String,
    pub ext_lines: BTreeMap<String, String>,
    pub dep_lines: BTreeMap<String, String>,
    pub ext_presets: BTreeMap<String, ExtDefault>,
    pub default_exts: BTreeMap<String, ExtDefault>,
}

/// 一个扩展默认配置：链接方式可选，feature 和依赖功能都是集合。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtDefault {
    /// 只用于 `[default_exts]`：引用 `ext_presets.<名字>`。
    #[serde(default, rename = "ref")]
    pub reference: String,
    /// 省略时按内置/非内置推导。
    #[serde(default)]
    pub link: Option<String>,
    /// 扩展自己的 feature，如 `["argon2"]`。
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

        let mut ext_lines = BTreeMap::new();
        let mut dep_lines = BTreeMap::new();
        let mut ext_presets = BTreeMap::new();
        let mut php = None;
        let mut default_exts = BTreeMap::new();
        for (key, value) in raw {
            if key == "php" {
                match value {
                    toml::Value::String(text) => php = Some(text),
                    _ => bail!("{} 的 php 必须是字符串", path.display()),
                }
            } else if key == "deps" {
                dep_lines = value.try_into().with_context(|| {
                    format!(
                        "{} 的 [deps] 必须是「依赖库名 -> 版本线字符串」",
                        path.display()
                    )
                })?;
            } else if key == "ext_presets" {
                ext_presets = value
                    .try_into()
                    .with_context(|| format!("{} 的 [ext_presets] 不合法", path.display()))?;
            } else if key == "default_exts" {
                default_exts = value
                    .try_into()
                    .with_context(|| format!("{} 的 [default_exts] 不合法", path.display()))?;
            } else {
                match value {
                    toml::Value::String(text) => {
                        ext_lines.insert(key, text);
                    }
                    _ => bail!(
                        "{} 的 {key} 必须是版本线字符串：扩展默认线写顶层，依赖库默认线写在 [deps]，\
                         扩展预设写在 [ext_presets]，默认扩展写在 [default_exts]",
                        path.display()
                    ),
                }
            }
        }

        let php = php.ok_or_else(|| anyhow::anyhow!("{} 缺少 php", path.display()))?;
        let bundle = Self {
            php,
            ext_lines,
            dep_lines,
            ext_presets,
            default_exts,
        };
        bundle.validate_refs(path)?;
        Ok(bundle)
    }

    fn validate_refs(&self, path: &Path) -> Result<()> {
        for (name, def) in &self.ext_presets {
            if name.is_empty() {
                bail!("{}: [ext_presets] 里有空名字", path.display());
            }
            if !def.reference.is_empty() {
                bail!("{}: ext_presets.{name} 不能写 ref", path.display());
            }
        }

        for (name, def) in &self.default_exts {
            if def.reference.is_empty() {
                continue;
            }
            let preset_name = match preset_name(&def.reference) {
                Ok(name) => name,
                Err(err) => bail!(
                    "{}: default_exts.{name} 的 ref 不合法：{err}",
                    path.display()
                ),
            };
            if preset_name != name {
                bail!(
                    "{}: default_exts.{name} 的 ref 必须指向 ext_presets.{name}",
                    path.display()
                );
            }
            if !self.ext_presets.contains_key(preset_name) {
                bail!(
                    "{}: default_exts.{name} 的 ref 指向不存在的 ext_presets.{preset_name}",
                    path.display()
                );
            }
            if !def.features.is_empty() || !def.deps.is_empty() {
                bail!(
                    "{}: default_exts.{name} 使用 ref 时不能再写 features/deps；\
                     需要不同配置时另建 preset",
                    path.display()
                );
            }
        }
        Ok(())
    }

    fn resolve_default(&self, name: &str) -> Result<ExtDefault> {
        let def = self
            .default_exts
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("[default_exts] 里没有 {name}"))?;
        if def.reference.is_empty() {
            return Ok(def.clone());
        }

        let preset_name = preset_name(&def.reference)?;
        let preset = self
            .ext_presets
            .get(preset_name)
            .ok_or_else(|| anyhow::anyhow!("default_exts.{name} 的 ref 指向不存在的 preset"))?;
        let mut merged = preset.clone();
        if let Some(link) = &def.link {
            merged.link = Some(link.clone());
        }
        Ok(merged)
    }

    /// `[default_exts]` 解析 ref 后实际使用的 feature。
    pub fn default_features(&self, name: &str) -> Option<&[String]> {
        let def = self.default_exts.get(name)?;
        if def.reference.is_empty() {
            Some(&def.features)
        } else {
            let preset_name = preset_name(&def.reference).ok()?;
            self.ext_presets
                .get(preset_name)
                .map(|preset| preset.features.as_slice())
        }
    }

    /// `[ext_presets]` 里的 feature；不解析 ref。
    pub fn preset_features(&self, name: &str) -> Option<&[String]> {
        self.ext_presets
            .get(name)
            .map(|preset| preset.features.as_slice())
    }

    pub fn check_series(&self, series: &str) -> Result<()> {
        if self.php != series {
            bail!("bundle 的 php={} 与当前系列 {series} 不一致", self.php);
        }
        Ok(())
    }
}

fn preset_name(reference: &str) -> Result<&str> {
    let Some(name) = reference.strip_prefix("ext_presets.") else {
        bail!("ref 必须是 \"ext_presets.<名字>\"，例如 \"ext_presets.gd\"");
    };
    if name.is_empty() || name.contains('.') {
        bail!("ref 必须是 \"ext_presets.<名字>\"，例如 \"ext_presets.gd\"");
    }
    Ok(name)
}

/// 把 bundle 的 `[default_exts]` 转成与 `--ext` 一致的 Selection。
///
/// 当前 PHP / 依赖线不满足的 feature 只提示并丢弃；写错依赖或 feature 名直接报错。
pub fn selection(reg: &Registry, bundle: &Bundle, series: &str) -> Result<Selection> {
    let mut sel = Selection::default();
    for name in bundle.default_exts.keys() {
        let def = bundle.resolve_default(name)?;
        let spec = build_spec(reg, name, &def, series, "[default_exts]")?;
        sel.per_ext.insert(name.clone(), spec);
    }
    Ok(sel)
}

/// 把 bundle 的 `[ext_presets]` 转成与 `--ext` 一致的 Selection。
///
/// 只提供预设，不会把扩展加入默认方案。
pub fn presets(reg: &Registry, bundle: &Bundle, series: &str) -> Result<Selection> {
    let mut sel = Selection::default();
    for (name, def) in &bundle.ext_presets {
        let spec = build_spec(reg, name, def, series, "[ext_presets]")?;
        sel.per_ext.insert(name.clone(), spec);
    }
    Ok(sel)
}

fn build_spec(
    reg: &Registry,
    name: &str,
    def: &ExtDefault,
    series: &str,
    table: &str,
) -> Result<ExtSpec> {
    let ext = reg.ext(name)?;
    let mut spec = ExtSpec::default();

    if let Some(link) = &def.link {
        let mode = Mode::parse(link)?;
        if !ext.link.iter().any(|m| m == mode.as_str()) {
            bail!(
                "bundle php{series}.toml 的 {table} 里 ext:{name} 不支持 link={link}\
                 （支持：{}）",
                ext.link.join(",")
            );
        }
        spec.link = Some(mode);
    }

    let mut features = BTreeSet::new();
    for f in &def.features {
        if !ext.features.contains_key(f) {
            eprintln!(
                "注意: bundle php{series}.toml 的 {table} 里 ext:{name} 没有 feature {f}，已丢弃"
            );
            continue;
        }
        if let Err(err) = crate::recipe::feature_available(reg, ext, f) {
            eprintln!("注意: bundle php{series}.toml 的 {table} 丢弃 {err}");
            continue;
        }
        features.insert(f.clone());
    }
    spec.features = Some(features);

    for (dep, feats) in &def.deps {
        if !ext.dep_configure.contains_key(dep) {
            bail!("bundle php{series}.toml 的 {table} 里 ext:{name} 没有依赖集成 dep:{dep}");
        }
        let target = reg
            .get(&format!("dep:{dep}"))
            .ok_or_else(|| anyhow::anyhow!("ext:{name} 的 dep:{dep} 指向不存在的依赖"))?;
        let mut available = Vec::new();
        for f in feats {
            if !target.features.contains_key(f) {
                bail!("bundle php{series}.toml 的 {table} 里 dep:{dep} 没有 feature {f}");
            }
            if let Err(err) = crate::recipe::feature_available(reg, target, f) {
                eprintln!("注意: bundle php{series}.toml 的 {table} 丢弃 {err}");
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
    fn default_exts_is_a_table_of_features_and_deps() {
        let bundle = Bundle::load(&dir().join("bundles/php8.5.toml")).unwrap();
        let openssl = &bundle.default_exts["openssl"];
        assert_eq!(openssl.features, vec!["argon2".to_owned()]);
        assert!(openssl.deps.is_empty());
        assert_eq!(openssl.link, None);

        let curl = &bundle.default_exts["curl"];
        assert!(curl.features.is_empty());
        assert_eq!(curl.deps["libcurl"], vec!["ssl".to_owned()]);
        assert_eq!(bundle.ext_lines["swoole"], "6.2");
        assert_eq!(bundle.dep_lines["openssl"], "3.5");
        assert!(!bundle.ext_lines.contains_key("openssl"));

        let gd = &bundle.ext_presets["gd"];
        assert_eq!(
            gd.features,
            vec!["jpeg".to_owned(), "webp".to_owned(), "freetype".to_owned()]
        );
        assert!(gd.reference.is_empty());
        assert!(!bundle.default_exts.contains_key("gd"));
    }

    #[test]
    fn other_series_have_no_default_exts() {
        let bundle = Bundle::load(&dir().join("bundles/php7.4.toml")).unwrap();
        assert!(bundle.default_exts.is_empty());
        assert!(bundle.ext_presets.is_empty());
    }

    #[test]
    fn ref_uses_preset_and_allows_link_override() {
        let path = temp_path("ref");
        std::fs::write(
            &path,
            r#"
php = "8.5"
[ext_presets]
gd = { features = ["jpeg"] }
[default_exts]
gd = { ref = "ext_presets.gd", link = "shared" }
"#,
        )
        .unwrap();
        let bundle = Bundle::load(&path).unwrap();
        let default = bundle.resolve_default("gd").unwrap();
        assert_eq!(default.features, vec!["jpeg".to_owned()]);
        assert_eq!(default.link.as_deref(), Some("shared"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_without_ref_ignores_same_named_preset() {
        let path = temp_path("local");
        std::fs::write(
            &path,
            r#"
php = "8.5"
[ext_presets]
gd = { features = ["jpeg"] }
[default_exts]
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
                r#"php = "8.5"
[ext_presets]
gd = { features = ["jpeg"] }
[default_exts]
curl = { ref = "ext_presets.gd" }
"#,
                "ref 必须指向 ext_presets.curl",
            ),
            (
                "mixed",
                r#"php = "8.5"
[ext_presets]
gd = { features = ["jpeg"] }
[default_exts]
gd = { ref = "ext_presets.gd", features = ["webp"] }
"#,
                "使用 ref 时不能再写 features/deps",
            ),
            (
                "missing",
                r#"php = "8.5"
[default_exts]
gd = { ref = "ext_presets.gd" }
"#,
                "指向不存在的 ext_presets.gd",
            ),
            (
                "badpath",
                r#"php = "8.5"
[default_exts]
gd = { ref = "gd" }
"#,
                "ref 必须是 \"ext_presets.<名字>\"",
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
            "php = \"8.5\"\nopenssl = { features = [\"argon2\"] }\n",
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
            "php = \"8.5\"\n[deps]\nopenssl = { line = \"3.5\" }\n",
        )
        .unwrap();
        let err = Bundle::load(&path).unwrap_err().to_string();
        assert!(err.contains("[deps]"), "{err}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unknown_default_ext_field_is_an_error() {
        let path = temp_path("unknown");
        std::fs::write(
            &path,
            "php = \"8.5\"\n[default_exts]\ncurl = { dep = { libcurl = [\"ssl\"] } }\n",
        )
        .unwrap();
        let err = Bundle::load(&path).unwrap_err().to_string();
        assert!(err.contains("[default_exts]"), "{err}");
        let _ = std::fs::remove_file(&path);
    }
}
