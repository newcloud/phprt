use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

/// catalog 里的一个具体补丁：版本、下载地址和校验和。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Artifact {
    pub version: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
}

/// `recipes/catalog.toml`：组件名 -> 版本线 -> 这条线的全部补丁。
///
/// 一条线对应一个数组，数组里是该线发布过的所有补丁；默认选择取其中最大的
/// 版本，顺序无关。`--lock` 和显式补丁按版本精确查找。
/// 这里不判断产品兼容性，也不存构建参数。
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub components: BTreeMap<String, BTreeMap<String, Vec<Artifact>>>,
}

impl Catalog {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            fs::read_to_string(path).with_context(|| format!("读不到 {}", path.display()))?;
        Self::parse(&text, &path.display().to_string())
    }

    pub(crate) fn parse(text: &str, origin: &str) -> Result<Self> {
        let components: BTreeMap<String, BTreeMap<String, Vec<Artifact>>> =
            toml::from_str(text).with_context(|| format!("{origin} 不是合法的 catalog.toml"))?;
        let catalog = Self { components };
        catalog.validate()?;
        Ok(catalog)
    }

    /// 源 catalog 打底，叠加本地在线检查取到的组件分片。
    ///
    /// `components_dir` 是 `<internal>/catalog/components`；不存在时等同于只读源文件。
    /// 本地分片按组件、按线追加；同一版本出现两个不同哈希时直接报错。
    pub fn load_layered(path: &Path, components_dir: &Path) -> Result<Self> {
        let mut catalog = Self::load(path)?;
        if !components_dir.is_dir() {
            return Ok(catalog);
        }
        let mut files: Vec<PathBuf> = fs::read_dir(components_dir)
            .with_context(|| format!("读不到 {}", components_dir.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("读不到 {}", components_dir.display()))?;
        files.retain(|path| path.extension().is_some_and(|ext| ext == "toml"));
        files.sort();

        for file in files {
            let text = fs::read_to_string(&file)
                .with_context(|| format!("读不到本地 catalog 分片 {}", file.display()))?;
            let shard = Self::parse(&text, &file.display().to_string())?;
            for (name, lines) in shard.components {
                for (line, artifacts) in &lines {
                    for artifact in artifacts {
                        if artifact.sha256.trim().is_empty() {
                            bail!(
                                "本地 catalog 分片 {} 的 {name}.{line}.{} 缺少 sha256；\
                                 删掉这个分片后重新检查",
                                file.display(),
                                artifact.version
                            );
                        }
                    }
                }
                catalog.merge_component(&name, lines, Some(&file))?;
            }
        }
        Ok(catalog)
    }

    /// 把一个组件分片追加到当前 catalog；同版本必须完全一致。
    pub fn merge_component(
        &mut self,
        name: &str,
        lines: BTreeMap<String, Vec<Artifact>>,
        source: Option<&Path>,
    ) -> Result<()> {
        let where_from = source
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "在线分片".to_owned());
        let target = self.components.entry(name.to_owned()).or_default();
        for (line, artifacts) in lines {
            let current = target.entry(line.clone()).or_default();
            for artifact in artifacts {
                if let Some(existing) = current
                    .iter_mut()
                    .find(|existing| existing.version == artifact.version)
                {
                    if existing.url != artifact.url {
                        bail!(
                            "本地 catalog 和 {where_from} 冲突：{name}.{line}.{} \
                             的 url 不一致；删掉这个分片后重新检查",
                            artifact.version
                        );
                    }
                    // 源 catalog 常常只记 url，不带 sha256；发布分片可以把它补上。
                    if existing.sha256.trim().is_empty() {
                        existing.sha256 = artifact.sha256;
                    } else if !existing.sha256.eq_ignore_ascii_case(&artifact.sha256) {
                        bail!(
                            "本地 catalog 和 {where_from} 冲突：{name}.{line}.{} \
                             的 sha256 不一致；删掉这个分片后重新检查",
                            artifact.version
                        );
                    }
                } else {
                    current.push(artifact);
                }
            }
        }
        Ok(())
    }

    pub fn lines(&self, name: &str) -> Option<&BTreeMap<String, Vec<Artifact>>> {
        self.components.get(name)
    }

    /// 这条线默认用哪个补丁：数组里版本最大的那个。
    pub fn get(&self, name: &str, line: &str) -> Option<&Artifact> {
        self.lines(name)?
            .get(line)?
            .iter()
            .max_by(|left, right| phprt_upstream::compare(&left.version, &right.version))
    }

    /// 线里的一个具体补丁；找不到返回 None。
    pub fn artifact_for(&self, name: &str, line: &str, version: &str) -> Option<Artifact> {
        self.lines(name)?
            .get(line)?
            .iter()
            .find(|artifact| artifact.version == version)
            .cloned()
    }

    /// 目标写了补丁就用补丁，只写线时用这条线的最大补丁。
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
                anyhow!(
                    "catalog 里没有 {product} {spec}（{series} 线可用：{}）",
                    self.version_list(product, series)
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
            .find(|(_, artifacts)| artifacts.iter().any(|artifact| artifact.version == spec))
        {
            return Ok(series.clone());
        }
        let available: Vec<String> = lines
            .iter()
            .map(|(series, artifacts)| {
                let versions: Vec<&str> = artifacts
                    .iter()
                    .map(|artifact| artifact.version.as_str())
                    .collect();
                format!("{series}（{}）", versions.join("/"))
            })
            .collect();
        bail!(
            "未知 {product} 版本 {spec}（可用：{}）",
            available.join(", ")
        )
    }

    fn version_list(&self, name: &str, line: &str) -> String {
        self.lines(name)
            .and_then(|lines| lines.get(line))
            .map(|artifacts| {
                artifacts
                    .iter()
                    .map(|artifact| artifact.version.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| "无".to_owned())
    }

    fn validate(&self) -> Result<()> {
        if self.components.is_empty() {
            bail!("catalog.toml 里没有任何组件");
        }
        for (name, lines) in &self.components {
            if lines.is_empty() {
                bail!("catalog 里的 {name} 没有任何版本线");
            }
            for (line, artifacts) in lines {
                if line.is_empty() {
                    bail!("catalog 里的 {name} 有空版本线");
                }
                if artifacts.is_empty() {
                    bail!("catalog 里的 {name}.{line} 没有任何补丁");
                }
                let mut seen = std::collections::BTreeSet::new();
                for artifact in artifacts {
                    if artifact.version.is_empty() {
                        bail!("catalog 里的 {name}.{line} 有补丁缺少 version");
                    }
                    if artifact.url.is_empty() {
                        bail!("catalog 里的 {name}.{line}.{} 缺少 url", artifact.version);
                    }
                    if !seen.insert(artifact.version.as_str()) {
                        bail!(
                            "catalog 里的 {name}.{line} 补丁 {version} 重复",
                            version = artifact.version
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
        let components: BTreeMap<String, BTreeMap<String, Vec<Artifact>>> =
            toml::from_str(text).unwrap();
        let catalog = Catalog { components };
        catalog.validate().unwrap();
        catalog
    }

    const PHP: &str = r#"
[php]
"8.4" = [
    { version = "8.4.14", url = "https://example.com/8.4.14.tar.gz" },
    { version = "8.4.13", url = "https://example.com/8.4.13.tar.gz" },
    { version = "8.4.12", url = "https://example.com/8.4.12.tar.gz" },
]
"#;

    #[test]
    fn line_takes_the_maximum_patch_regardless_of_order() {
        let text = r#"
[openssl]
"3.5" = [
    { version = "3.5.6", url = "https://example.com/3.5.6.tar.gz" },
    { version = "3.5.9", url = "https://example.com/3.5.9.tar.gz" },
    { version = "3.5.8", url = "https://example.com/3.5.8.tar.gz" },
]
"#;
        let catalog = catalog(text);
        assert_eq!(
            catalog
                .product_version_for("openssl", "3.5", "3.5")
                .unwrap(),
            "3.5.9"
        );
    }

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
"8.4" = [
    { version = "8.4.14", url = "u" },
    { version = "8.4.14", url = "u" },
]
"#;
        let components: BTreeMap<String, BTreeMap<String, Vec<Artifact>>> =
            toml::from_str(text).unwrap();
        assert!(Catalog { components }.validate().is_err());
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "phprt-catalog-{tag}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_text(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn local_shard_appends_to_the_source_catalog() {
        let root = temp_dir("merge");
        let source = root.join("catalog.toml");
        write_text(&source, PHP);
        let components = root.join("components");
        write_text(
            &components.join("php.toml"),
            r#"
[php]
"8.4" = [ { version = "8.4.28", url = "https://example.com/8.4.28.tar.gz", sha256 = "aa" } ]
"#,
        );
        let catalog = Catalog::load_layered(&source, &components).unwrap();
        assert_eq!(catalog.get("php", "8.4").unwrap().version, "8.4.28");
        assert!(catalog.artifact_for("php", "8.4", "8.4.14").is_some());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn local_shard_does_not_downgrade_a_newer_source_patch() {
        let root = temp_dir("downgrade");
        let source = root.join("catalog.toml");
        write_text(&source, PHP);
        let components = root.join("components");
        write_text(
            &components.join("php.toml"),
            r#"
[php]
"8.4" = [ { version = "8.4.13", url = "https://example.com/8.4.13.tar.gz", sha256 = "bb" } ]
"#,
        );
        let catalog = Catalog::load_layered(&source, &components).unwrap();
        assert_eq!(catalog.get("php", "8.4").unwrap().version, "8.4.14");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn local_shard_conflict_is_an_error() {
        let root = temp_dir("conflict");
        let source = root.join("catalog.toml");
        write_text(&source, PHP);
        let components = root.join("components");
        write_text(
            &components.join("php.toml"),
            r#"
[php]
"8.4" = [ { version = "8.4.14", url = "https://other.example/8.4.14.tar.gz", sha256 = "cc" } ]
"#,
        );
        let err = Catalog::load_layered(&source, &components)
            .unwrap_err()
            .to_string();
        assert!(err.contains("冲突"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn local_shard_requires_sha256() {
        let root = temp_dir("missing-sha");
        let source = root.join("catalog.toml");
        write_text(&source, PHP);
        let components = root.join("components");
        write_text(
            &components.join("php.toml"),
            r#"
[php]
"8.4" = [ { version = "8.4.28", url = "https://example.com/8.4.28.tar.gz" } ]
"#,
        );
        let err = Catalog::load_layered(&source, &components)
            .unwrap_err()
            .to_string();
        assert!(err.contains("缺少 sha256"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn local_shard_can_fill_a_missing_source_hash() {
        let root = temp_dir("fill-hash");
        let source = root.join("catalog.toml");
        write_text(&source, PHP);
        let components = root.join("components");
        write_text(
            &components.join("php.toml"),
            r#"
[php]
"8.4" = [ { version = "8.4.14", url = "https://example.com/8.4.14.tar.gz", sha256 = "dd" } ]
"#,
        );
        let catalog = Catalog::load_layered(&source, &components).unwrap();
        let artifact = catalog.artifact_for("php", "8.4", "8.4.14").unwrap();
        assert_eq!(artifact.sha256, "dd");
        let _ = fs::remove_dir_all(&root);
    }
}
