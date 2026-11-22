use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::types::{ImageInfo, InstanceMeta};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct Index {
    #[serde(default)]
    images: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    digest: String,
    #[serde(default)]
    refs: Vec<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    platform: String,
    #[serde(default)]
    created: String,
    #[serde(default)]
    size: u64,
}

/// 用户级镜像仓库。
///
/// 默认 `~/.local/share/phprt/buildimages`；测试或特殊环境可以用
/// `PHPRT_BUILDIMAGES_DIR` 覆盖。
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open() -> Result<Self> {
        let root = match std::env::var_os("PHPRT_BUILDIMAGES_DIR") {
            Some(value) if !value.is_empty() => PathBuf::from(value),
            _ => {
                let base = match std::env::var_os("XDG_DATA_HOME") {
                    Some(value) if !value.is_empty() => PathBuf::from(value),
                    _ => {
                        let home = std::env::var_os("HOME")
                            .ok_or_else(|| anyhow!("读不到 HOME，无法定位构建镜像目录"))?;
                        PathBuf::from(home).join(".local/share")
                    }
                };
                base.join("phprt/buildimages")
            }
        };
        fs::create_dir_all(root.join("images"))
            .with_context(|| format!("创建 {}", root.join("images").display()))?;
        fs::create_dir_all(root.join("instances"))
            .with_context(|| format!("创建 {}", root.join("instances").display()))?;
        Ok(Self { root })
    }

    /// 测试或嵌调用时直接指定根目录。
    pub fn at(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(root.join("images"))?;
        fs::create_dir_all(root.join("instances"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn images_dir(&self) -> PathBuf {
        self.root.join("images")
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.root.join("instances")
    }

    pub fn image_dir(&self, digest: &str) -> PathBuf {
        self.images_dir().join(digest_hex(digest))
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join("index.toml")
    }

    pub fn list(&self) -> Result<Vec<ImageInfo>> {
        let index = self.read_index()?;
        let mut out = Vec::new();
        for entry in index.images {
            let dir = self.image_dir(&entry.digest);
            if !dir.join("rootfs").is_dir() {
                continue;
            }
            out.push(ImageInfo {
                digest: entry.digest,
                reference: entry.refs.first().cloned().unwrap_or_default(),
                refs: entry.refs,
                aliases: entry.aliases,
                platform: entry.platform,
                created: entry.created,
                size: entry.size,
                dir,
            });
        }
        out.sort_by(|a, b| a.digest.cmp(&b.digest));
        Ok(out)
    }

    /// 按 digest / 别名 / 镜像引用查一个已经缓存好的镜像。
    pub fn resolve(&self, query: &str) -> Result<ImageInfo> {
        let index = self.read_index()?;
        let normalized = normalize_digest(query);
        let entry = index
            .images
            .iter()
            .find(|entry| {
                entry.digest == query
                    || normalized
                        .as_deref()
                        .is_some_and(|digest| entry.digest == digest)
                    || entry.aliases.iter().any(|alias| alias == query)
                    || entry.refs.iter().any(|reference| reference == query)
            })
            .ok_or_else(|| {
                let known: Vec<String> = index
                    .images
                    .iter()
                    .flat_map(|entry| {
                        let mut names = vec![short_digest(&entry.digest)];
                        names.extend(entry.aliases.iter().cloned());
                        names
                    })
                    .collect();
                if known.is_empty() {
                    anyhow!("本地还没有构建镜像；先执行 phprt buildimages add {query}")
                } else {
                    anyhow!(
                        "本地没有镜像 {query}（已缓存：{}）；先执行 phprt buildimages add {query}",
                        known.join(", ")
                    )
                }
            })?;
        let dir = self.image_dir(&entry.digest);
        if !dir.join("rootfs").is_dir() {
            bail!(
                "镜像 {} 的元数据还在，但 rootfs 已丢失；重新执行 phprt buildimages add {}",
                short_digest(&entry.digest),
                entry.refs.first().map(String::as_str).unwrap_or(query)
            );
        }
        Ok(ImageInfo {
            digest: entry.digest.clone(),
            reference: entry.refs.first().cloned().unwrap_or_default(),
            refs: entry.refs.clone(),
            aliases: entry.aliases.clone(),
            platform: entry.platform.clone(),
            created: entry.created.clone(),
            size: entry.size,
            dir,
        })
    }

    pub(crate) fn read_index(&self) -> Result<Index> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(Index::default());
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("读不到 {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("{} 不是合法的索引", path.display()))
    }

    pub(crate) fn write_index(&self, index: &Index) -> Result<()> {
        let path = self.index_path();
        let text = toml::to_string_pretty(index)?;
        write_atomic(&path, text.as_bytes())
    }

    /// 记录一次成功拉取 / 解包。
    pub(crate) fn record(
        &self,
        digest: &str,
        reference: &str,
        alias: Option<&str>,
        platform: &str,
        created: &str,
        size: u64,
    ) -> Result<()> {
        let mut index = self.read_index()?;
        if let Some(alias) = alias
            && let Some(other) = index.images.iter().find(|entry| {
                entry.digest != digest && entry.aliases.iter().any(|item| item == alias)
            })
        {
            bail!(
                "别名 {alias} 已经指向 {}；换一个名字",
                short_digest(&other.digest)
            );
        }
        if !reference.is_empty() {
            // 同一个 tag 前进后，ref 只指向最新 digest；旧内容仍可按 digest 使用。
            for other in index
                .images
                .iter_mut()
                .filter(|other| other.digest != digest)
            {
                other.refs.retain(|item| item != reference);
            }
        }
        let entry = match index.images.iter_mut().find(|entry| entry.digest == digest) {
            Some(entry) => entry,
            None => {
                index.images.push(Entry {
                    digest: digest.to_owned(),
                    refs: Vec::new(),
                    aliases: Vec::new(),
                    platform: platform.to_owned(),
                    created: created.to_owned(),
                    size,
                });
                index.images.last_mut().expect("just pushed")
            }
        };
        if !reference.is_empty() && !entry.refs.iter().any(|item| item == reference) {
            entry.refs.push(reference.to_owned());
        }
        if let Some(alias) = alias
            && !entry.aliases.iter().any(|item| item == alias)
        {
            entry.aliases.push(alias.to_owned());
        }
        entry.platform = platform.to_owned();
        entry.created = created.to_owned();
        entry.size = size;
        self.write_index(&index)
    }

    /// 删除镜像。`referenced` 是当前项目 state 里引用的 digest。
    pub fn remove(&self, query: &str, referenced: &[String], force: bool) -> Result<Vec<String>> {
        let image = self.resolve(query)?;
        if !force && referenced.iter().any(|digest| digest == &image.digest) {
            bail!(
                "镜像 {} 正被当前项目的 state.json 引用；确要删除请加 --force",
                short_digest(&image.digest)
            );
        }
        fs::remove_dir_all(&image.dir).with_context(|| format!("删除 {}", image.dir.display()))?;
        let mut index = self.read_index()?;
        index.images.retain(|entry| entry.digest != image.digest);
        self.write_index(&index)?;
        Ok(self.instances_using(&image.digest))
    }

    fn instances_using(&self, digest: &str) -> Vec<String> {
        let dir = self.instances_dir();
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(&dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let meta = entry.path().join("instance.json");
            let Ok(text) = fs::read_to_string(&meta) else {
                continue;
            };
            let Ok(meta) = serde_json::from_str::<InstanceMeta>(&text) else {
                continue;
            };
            if meta.image == digest {
                out.push(meta.id);
            }
        }
        out
    }

    /// 清理：项目根已经不存在的实例，以及索引里没有的镜像目录。
    ///
    /// 返回被删除（或 dry-run 时将要删除）的路径。
    pub fn gc(&self, dry_run: bool) -> Result<Vec<PathBuf>> {
        let mut removed = Vec::new();
        let index = self.read_index()?;
        let known: Vec<String> = index.images.iter().map(|e| e.digest.clone()).collect();

        if let Ok(entries) = fs::read_dir(self.images_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let digest = format!("sha256:{name}");
                if !known.iter().any(|item| item == &digest) {
                    removed.push(path.clone());
                    if !dry_run {
                        let _ = fs::remove_dir_all(&path);
                    }
                }
            }
        }

        if let Ok(entries) = fs::read_dir(self.instances_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                let meta_path = path.join("instance.json");
                let Ok(text) = fs::read_to_string(&meta_path) else {
                    continue;
                };
                let Ok(meta) = serde_json::from_str::<InstanceMeta>(&text) else {
                    continue;
                };
                if !Path::new(&meta.project_root).is_dir() {
                    removed.push(path.clone());
                    if !dry_run {
                        crate::runtime::delete_instance(&meta.id).ok();
                        let _ = fs::remove_dir_all(&path);
                    }
                }
            }
        }
        Ok(removed)
    }
}

pub fn short_digest(digest: &str) -> String {
    digest
        .strip_prefix("sha256:")
        .unwrap_or(digest)
        .chars()
        .take(12)
        .collect()
}

pub fn digest_hex(digest: &str) -> String {
    digest.strip_prefix("sha256:").unwrap_or(digest).to_owned()
}

pub fn normalize_digest(value: &str) -> Option<String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    if hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("sha256:{}", hex.to_ascii_lowercase()))
    } else {
        None
    }
}

pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size(&path);
        } else {
            total += meta.len();
        }
    }
    total
}

pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data).with_context(|| format!("写入 {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("写入 {}", path.display()))?;
    Ok(())
}
