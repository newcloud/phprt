use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// catalog 里一条版本线的当前最新补丁。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Artifact {
    pub version: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

/// `recipes/catalog.toml`：组件名 -> 版本线 -> 最新补丁。
///
/// 这里不判断 PHP 兼容性，也不存构建参数。
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub components: BTreeMap<String, BTreeMap<String, Artifact>>,
}

impl Catalog {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("读不到 {}", path.display()))?;
        let components: BTreeMap<String, BTreeMap<String, Artifact>> = toml::from_str(&text)
            .with_context(|| format!("{} 不是合法的 catalog.toml", path.display()))?;
        let catalog = Self { components };
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn lines(&self, name: &str) -> Option<&BTreeMap<String, Artifact>> {
        self.components.get(name)
    }

    pub fn get(&self, name: &str, line: &str) -> Option<&Artifact> {
        self.lines(name).and_then(|lines| lines.get(line))
    }

    pub fn php_series(&self) -> Vec<String> {
        self.lines("php")
            .map(|lines| lines.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn php_version(&self, series: &str) -> Result<&str> {
        self.get("php", series)
            .map(|a| a.version.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "catalog 里没有 PHP {}（可用：{}）",
                    series,
                    self.php_series().join(", ")
                )
            })
    }

    pub fn highest_php_series(&self) -> Result<String> {
        self.php_series()
            .into_iter()
            .next_back()
            .ok_or_else(|| anyhow::anyhow!("catalog 里没有 php 版本"))
    }

    /// 接受 `8.5`、`8.5.12`、`php:8.5.12`，返回系列。
    pub fn find_php_series(&self, spec: &str) -> Result<String> {
        let spec = spec.strip_prefix("php:").unwrap_or(spec);
        let lines = self
            .lines("php")
            .ok_or_else(|| anyhow::anyhow!("catalog 里没有 php 组件"))?;

        if lines.contains_key(spec) {
            return Ok(spec.to_owned());
        }
        if let Some((series, _)) = lines.iter().find(|(_, a)| a.version == spec) {
            return Ok(series.clone());
        }
        let available: Vec<String> = lines
            .iter()
            .map(|(series, a)| format!("{series}（{}）", a.version))
            .collect();
        bail!("未知 PHP 版本 {spec}（可用：{}）", available.join(", "))
    }

    fn validate(&self) -> Result<()> {
        if !self.components.contains_key("php") {
            bail!("catalog.toml 缺少 [php]");
        }
        for (name, lines) in &self.components {
            if lines.is_empty() {
                bail!("catalog 里的 {name} 没有任何版本线");
            }
            for (line, artifact) in lines {
                if line.is_empty() {
                    bail!("catalog 里的 {name} 有空版本线");
                }
                if artifact.version.is_empty() {
                    bail!("catalog 里的 {name}.{line} 缺少 version");
                }
                if artifact.url.is_empty() {
                    bail!("catalog 里的 {name}.{line} 缺少 url");
                }
            }
        }
        Ok(())
    }
}
