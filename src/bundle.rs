use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::recipe::{ExtSpec, Mode, Registry, Selection};

/// `recipes/bundles/php<系列>.toml`：某个 PHP 系列的默认策略。
///
/// - 顶层：有独立版本线的扩展 -> 默认线；只存线，不存补丁号。
/// - `[deps]`：依赖库 -> 默认线。内置扩展没有自己的版本线，它的同名库写在这里。
/// - `[default_exts]`：默认启用的扩展，以及它们选中的 feature / 依赖功能。
#[derive(Debug, Clone)]
pub struct Bundle {
    pub php: String,
    pub ext_lines: BTreeMap<String, String>,
    pub dep_lines: BTreeMap<String, String>,
    pub default_exts: BTreeMap<String, ExtDefault>,
}

/// 一个默认扩展：链接方式可选，feature 和依赖功能都是集合。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtDefault {
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
                         默认扩展写在 [default_exts]",
                        path.display()
                    ),
                }
            }
        }

        let php = php.ok_or_else(|| anyhow::anyhow!("{} 缺少 php", path.display()))?;
        Ok(Self {
            php,
            ext_lines,
            dep_lines,
            default_exts,
        })
    }

    pub fn check_series(&self, series: &str) -> Result<()> {
        if self.php != series {
            bail!("bundle 的 php={} 与当前系列 {series} 不一致", self.php);
        }
        Ok(())
    }
}

/// 把 bundle 的 `[default_exts]` 转成与 `--ext` 一致的 Selection。
///
/// 当前 PHP / 依赖线不满足的 feature 只提示并丢弃；写错依赖或 feature 名直接报错。
pub fn selection(reg: &Registry, bundle: &Bundle, series: &str) -> Result<Selection> {
    let mut sel = Selection::default();
    for (name, def) in &bundle.default_exts {
        let ext = reg.ext(name)?;
        let mut spec = ExtSpec::default();

        if let Some(link) = &def.link {
            let mode = Mode::parse(link)?;
            if !ext.link.iter().any(|m| m == mode.as_str()) {
                bail!(
                    "bundle php{series}.toml 的 [default_exts] 里 ext:{name} 不支持 link={link}\
                     （支持：{}）",
                    ext.link.join(",")
                );
            }
            spec.link = Some(mode);
        }

        let mut features = std::collections::BTreeSet::new();
        for f in &def.features {
            if !ext.features.contains_key(f) {
                eprintln!(
                    "注意: bundle php{series}.toml 的 default_exts 里 ext:{name} 没有 feature {f}，已丢弃"
                );
                continue;
            }
            if let Err(err) = crate::recipe::feature_available(reg, ext, f) {
                eprintln!("注意: bundle php{series}.toml 的 default_exts 丢弃 {err}");
                continue;
            }
            features.insert(f.clone());
        }
        spec.features = Some(features);

        for (dep, feats) in &def.deps {
            if !ext.dep_configure.contains_key(dep) {
                bail!(
                    "bundle php{series}.toml 的 default_exts 里 ext:{name} 没有依赖集成 dep:{dep}"
                );
            }
            let target = reg
                .get(&format!("dep:{dep}"))
                .ok_or_else(|| anyhow::anyhow!("ext:{name} 的 dep:{dep} 指向不存在的依赖"))?;
            for f in feats {
                if !target.features.contains_key(f) {
                    bail!("bundle php{series}.toml 的 default_exts 里 dep:{dep} 没有 feature {f}");
                }
            }
            spec.deps.insert(dep.clone(), feats.clone());
        }

        sel.per_ext.insert(name.clone(), spec);
    }
    Ok(sel)
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
    }

    #[test]
    fn other_series_have_no_default_exts() {
        let bundle = Bundle::load(&dir().join("bundles/php7.4.toml")).unwrap();
        assert!(bundle.default_exts.is_empty());
    }

    #[test]
    fn line_values_must_not_be_tables() {
        let path = std::env::temp_dir().join(format!("phprt-bundle-{}", std::process::id()));
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
        let path = std::env::temp_dir().join(format!("phprt-bundle-deps-{}", std::process::id()));
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
        let path = std::env::temp_dir().join(format!("phprt-bundle2-{}", std::process::id()));
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
