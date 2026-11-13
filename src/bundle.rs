use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

/// `recipes/bundles/php<系列>.toml`：某个 PHP 系列的默认策略。
///
/// - 行版本线：只存线，不存补丁号；补丁号来自 catalog。
/// - `default_exts`：默认启用的扩展规格，空格分隔；空/缺省表示不默认启用。
#[derive(Debug, Clone)]
pub struct Bundle {
    pub php: String,
    pub defaults: BTreeMap<String, String>,
    pub default_exts: Vec<String>,
}

impl Bundle {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("读不到 {}", path.display()))?;
        let mut map: BTreeMap<String, String> = toml::from_str(&text)
            .with_context(|| format!("{} 不是合法的 bundle", path.display()))?;
        let php = map
            .remove("php")
            .ok_or_else(|| anyhow::anyhow!("{} 缺少 php", path.display()))?;
        let default_exts = map
            .remove("default_exts")
            .map(|text| {
                text.split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(Self {
            php,
            defaults: map,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes")
    }

    #[test]
    fn default_exts_is_a_space_separated_string() {
        let bundle = Bundle::load(&dir().join("bundles/php8.5.toml")).unwrap();
        assert_eq!(bundle.default_exts, vec!["openssl", "curl=sub:curl[ssl]"]);
        assert_eq!(bundle.defaults["openssl"], "3.5");
    }

    #[test]
    fn other_series_have_no_default_exts() {
        let bundle = Bundle::load(&dir().join("bundles/php7.4.toml")).unwrap();
        assert!(bundle.default_exts.is_empty());
    }
}
