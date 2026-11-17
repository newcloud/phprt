use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

/// catalog 里一条版本线的当前最新补丁。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Artifact {
    pub version: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    /// 同一条线里发布过的其他补丁；当前补丁仍写在 version/url/sha256。
    #[serde(default)]
    pub patches: Vec<Patch>,
}

/// 版本线里的一个旧补丁。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Patch {
    pub version: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

/// `recipes/catalog.toml`：组件名 -> 版本线 -> 最新补丁。
///
/// 这里不判断产品兼容性，也不存构建参数。产品（php / nginx / cpython）、
/// 组件和依赖库都用同一张表。
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

    /// 线里的具体补丁：当前补丁或 patches 里的旧补丁。
    pub fn artifact_for(&self, name: &str, line: &str, version: &str) -> Option<Artifact> {
        let current = self.get(name, line)?;
        if current.version == version {
            return Some(Artifact {
                version: current.version.clone(),
                url: current.url.clone(),
                sha256: current.sha256.clone(),
                patches: Vec::new(),
            });
        }
        current
            .patches
            .iter()
            .find(|patch| patch.version == version)
            .map(|patch| Artifact {
                version: patch.version.clone(),
                url: patch.url.clone(),
                sha256: patch.sha256.clone(),
                patches: Vec::new(),
            })
    }

    /// 目标写了补丁就用补丁，只写线时用当前补丁。
    pub fn product_version_for(
        &self,
        product: &str,
        series: &str,
        requested: &str,
    ) -> Result<String> {
        let spec = requested
            .strip_prefix(&format!("{product}@"))
            .unwrap_or(requested);
        let current = self
            .get(product, series)
            .ok_or_else(|| anyhow!("catalog 里没有 {product} {series}"))?;
        if spec == series {
            return Ok(current.version.clone());
        }
        self.artifact_for(product, series, spec)
            .map(|artifact| artifact.version)
            .ok_or_else(|| {
                let mut versions = vec![current.version.clone()];
                versions.extend(current.patches.iter().map(|patch| patch.version.clone()));
                anyhow!(
                    "catalog 里没有 {product} {spec}（{series} 线可用：{}）",
                    versions.join(", ")
                )
            })
    }

    /// 接受 `8.5`、`8.5.12`、`php@8.5.12`，返回系列。
    pub fn find_product_series(&self, product: &str, spec: &str) -> Result<String> {
        let spec = spec.strip_prefix(&format!("{product}@")).unwrap_or(spec);
        let lines = self
            .lines(product)
            .ok_or_else(|| anyhow::anyhow!("catalog 里没有产品 {product}"))?;

        if lines.contains_key(spec) {
            return Ok(spec.to_owned());
        }
        if let Some((series, _)) = lines
            .iter()
            .find(|(_, a)| a.version == spec || a.patches.iter().any(|p| p.version == spec))
        {
            return Ok(series.clone());
        }
        let available: Vec<String> = lines
            .iter()
            .map(|(series, a)| {
                let mut text = format!("{series}（{}）", a.version);
                if !a.patches.is_empty() {
                    let old: Vec<&str> = a.patches.iter().map(|p| p.version.as_str()).collect();
                    text.push_str(&format!("，旧补丁 {}", old.join("/")));
                }
                text
            })
            .collect();
        bail!(
            "未知 {product} 版本 {spec}（可用：{}）",
            available.join(", ")
        )
    }

    fn validate(&self) -> Result<()> {
        if self.components.is_empty() {
            bail!("catalog.toml 里没有任何组件");
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
                let mut seen = std::collections::BTreeSet::new();
                seen.insert(artifact.version.as_str());
                for patch in &artifact.patches {
                    if patch.version.is_empty() {
                        bail!("catalog 里的 {name}.{line} 有补丁缺少 version");
                    }
                    if patch.url.is_empty() {
                        bail!("catalog 里的 {name}.{line}.{} 缺少 url", patch.version);
                    }
                    if !seen.insert(patch.version.as_str()) {
                        bail!(
                            "catalog 里的 {name}.{line} 补丁 {patch_version} 重复",
                            patch_version = patch.version
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(text: &str) -> Catalog {
        let components: BTreeMap<String, BTreeMap<String, Artifact>> =
            toml::from_str(text).unwrap();
        let catalog = Catalog { components };
        catalog.validate().unwrap();
        catalog
    }

    const PHP: &str = r#"
[php]
"8.4" = { version = "8.4.14", url = "https://example.com/8.4.14.tar.gz", patches = [
    { version = "8.4.13", url = "https://example.com/8.4.13.tar.gz" },
] }
"#;

    #[test]
    fn patch_targets_resolve_to_their_series() {
        let catalog = catalog(PHP);
        assert_eq!(catalog.find_product_series("php", "8.4.13").unwrap(), "8.4");
        assert_eq!(
            catalog.product_version_for("php", "8.4", "8.4").unwrap(),
            "8.4.14"
        );
        assert_eq!(
            catalog
                .product_version_for("php", "8.4", "php@8.4.13")
                .unwrap(),
            "8.4.13"
        );
        assert_eq!(
            catalog.artifact_for("php", "8.4", "8.4.13").unwrap().url,
            "https://example.com/8.4.13.tar.gz"
        );
    }

    #[test]
    fn unknown_patch_lists_available_versions() {
        let catalog = catalog(PHP);
        let err = catalog
            .product_version_for("php", "8.4", "8.4.9")
            .unwrap_err()
            .to_string();
        assert!(err.contains("8.4.14") && err.contains("8.4.13"), "{err}");
    }

    #[test]
    fn duplicate_patch_version_is_rejected() {
        let text = r#"
[php]
"8.4" = { version = "8.4.14", url = "u", patches = [
    { version = "8.4.14", url = "u" },
] }
"#;
        let components: BTreeMap<String, BTreeMap<String, Artifact>> =
            toml::from_str(text).unwrap();
        assert!(Catalog { components }.validate().is_err());
    }
}
