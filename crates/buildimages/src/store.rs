use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::types::{ImageInfo, InstanceInfo, InstanceMeta};
use crate::unpack::make_removable;

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

    pub fn instance_dir(&self, id: &str) -> PathBuf {
        self.instances_dir().join(id)
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

    /// 按 digest / digest 前缀 / 别名 / 镜像引用查一个已经缓存好的镜像。
    pub fn resolve(&self, query: &str) -> Result<ImageInfo> {
        let index = self.read_index()?;
        let normalized = normalize_digest(query);
        let exact = index.images.iter().find(|entry| {
            entry.digest == query
                || normalized
                    .as_deref()
                    .is_some_and(|digest| entry.digest == digest)
                || entry.aliases.iter().any(|alias| alias == query)
                || entry.refs.iter().any(|reference| reference == query)
        });
        let entry = match exact {
            Some(entry) => entry,
            None => {
                let prefix = query.strip_prefix("sha256:").unwrap_or(query);
                let prefix = prefix.to_ascii_lowercase();
                if prefix.len() < 8
                    || prefix.len() >= 64
                    || !prefix.chars().all(|c| c.is_ascii_hexdigit())
                {
                    return Err(missing_image(&index, query));
                }
                let mut matches = index
                    .images
                    .iter()
                    .filter(|entry| digest_hex(&entry.digest).starts_with(&prefix));
                let Some(entry) = matches.next() else {
                    return Err(missing_image(&index, query));
                };
                if let Some(other) = matches.next() {
                    bail!(
                        "digest 前缀 {query} 不唯一：{} 和 {}；多写几位",
                        short_digest(&entry.digest),
                        short_digest(&other.digest)
                    );
                }
                entry
            }
        };
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

    /// 列出本地全部实例。实例是全局对象，不按项目过滤。
    pub fn list_instances(&self) -> Result<Vec<InstanceInfo>> {
        let dir = self.instances_dir();
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(&dir) else {
            return Ok(out);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some((meta, bundle)) = read_instance_meta(&path) {
                out.push(instance_info(meta, bundle));
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    /// 按名字或 id 找实例；找不到返回 None。
    pub fn find_instance(&self, query: &str) -> Result<Option<InstanceInfo>> {
        let mut matches: Vec<InstanceInfo> = self
            .list_instances()?
            .into_iter()
            .filter(|instance| instance.name == query || instance.id == query)
            .collect();
        match matches.len() {
            0 => Ok(None),
            1 => Ok(matches.pop()),
            _ => {
                let names: Vec<String> = matches
                    .iter()
                    .map(|instance| format!("{} ({})", instance.name, instance.id))
                    .collect();
                bail!("实例名 {query} 同时匹配多个实例：{}", names.join(", "))
            }
        }
    }

    /// 按名字或 id 找实例；找不到时列出本地已有实例。
    pub fn resolve_instance(&self, query: &str) -> Result<InstanceInfo> {
        if let Some(instance) = self.find_instance(query)? {
            return Ok(instance);
        }
        let known: Vec<String> = self
            .list_instances()?
            .into_iter()
            .map(|instance| format!("{}（{}）", instance.name, short_digest(&instance.image)))
            .collect();
        if known.is_empty() {
            bail!(
                "本地还没有构建实例；先执行 phprt install <产品>@<系列> --image <镜像> --instance-name {query}"
            );
        }
        bail!(
            "本地没有实例 {query}（已有：{}）；\
             使用已有实例用 --instance <名字>，新建实例用 --image <镜像> --instance-name <名字>",
            known.join(", ")
        )
    }

    /// 返回引用某个镜像 digest 的实例 id。
    pub fn instances_using(&self, digest: &str) -> Vec<String> {
        self.list_instances()
            .unwrap_or_default()
            .into_iter()
            .filter(|instance| instance.image == digest)
            .map(|instance| instance.id)
            .collect()
    }

    /// 删除镜像。调用方负责 state 引用检查；这里硬性拒绝仍被实例使用的镜像。
    pub fn remove_image(&self, image: &ImageInfo) -> Result<()> {
        if let Ok(entries) = fs::read_dir(self.instances_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if (path.join("instance.json").exists() || path.join("bundle/rootfs").exists())
                    && read_instance_meta(&path).is_none()
                {
                    bail!(
                        "实例目录 {} 无法读取；先检查或手工清理，再删除镜像",
                        path.display()
                    );
                }
            }
        }
        let users: Vec<String> = self
            .list_instances()?
            .into_iter()
            .filter(|instance| instance.image == image.digest)
            .map(|instance| format!("{}（{}）", instance.name, instance.id))
            .collect();
        if !users.is_empty() {
            bail!(
                "镜像 {} 仍被以下实例使用：\n  {}\n\
                 先执行 phprt buildimages rm-instance <名字>，再删除镜像",
                short_digest(&image.digest),
                users.join("\n  ")
            );
        }
        make_removable(&image.dir)?;
        fs::remove_dir_all(&image.dir).with_context(|| format!("删除 {}", image.dir.display()))?;
        let mut index = self.read_index()?;
        index.images.retain(|entry| entry.digest != image.digest);
        self.write_index(&index)
    }

    /// 删除实例目录和运行时容器；调用方负责 state / 锁检查。
    pub fn remove_instance(&self, id: &str) -> Result<()> {
        let dir = self.instance_dir(id);
        if !dir.exists() {
            bail!("实例目录不存在：{}", dir.display());
        }
        crate::runtime::delete_instance(id)
            .with_context(|| format!("删除实例 {id} 的运行时容器失败"))?;
        make_removable(&dir)?;
        fs::remove_dir_all(&dir).with_context(|| format!("删除 {}", dir.display()))
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
}

fn missing_image(index: &Index, query: &str) -> anyhow::Error {
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
}

fn read_instance_meta(dir: &Path) -> Option<(InstanceMeta, PathBuf)> {
    let text = fs::read_to_string(dir.join("instance.json")).ok()?;
    let mut meta: InstanceMeta = serde_json::from_str(&text).ok()?;
    if meta.id.is_empty() {
        meta.id = dir.file_name()?.to_string_lossy().to_string();
    }
    let bundle = dir.join("bundle");
    Some((meta, bundle))
}

fn instance_info(meta: InstanceMeta, bundle: PathBuf) -> InstanceInfo {
    let projects = meta.project_roots();
    let owner = if meta.owner.is_empty() {
        projects.first().cloned().unwrap_or_default()
    } else {
        meta.owner.clone()
    };
    let name = meta.display_name();
    InstanceInfo {
        id: meta.id,
        name,
        image: meta.image,
        image_ref: meta.image_ref,
        owner,
        projects,
        created: meta.created,
        rootfs: bundle.join("rootfs"),
        bundle,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_meta(store: &Store, id: &str, json: &str) {
        let dir = store.instance_dir(id);
        fs::create_dir_all(dir.join("bundle/rootfs/usr")).unwrap();
        fs::write(dir.join("instance.json"), json).unwrap();
    }

    #[test]
    fn lists_and_resolves_named_and_legacy_instances() {
        let dir = std::env::temp_dir().join(format!("phprt-bi-inst-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::at(dir.clone()).unwrap();
        write_meta(
            &store,
            "phprt-4add57aa-php-8.5-abcdef123456",
            r#"{
              "id": "phprt-4add57aa-php-8.5-abcdef123456",
              "image": "sha256:aa",
              "project_root": "/tmp/a",
              "created": "now"
            }"#,
        );
        write_meta(
            &store,
            "phprt-fedora43",
            r#"{
              "id": "phprt-fedora43",
              "name": "fedora43",
              "image": "sha256:aa",
              "image_ref": "quay.io/fedora/fedora:43",
              "owner": "/tmp/b",
              "projects": ["/tmp/b"],
              "created": "now"
            }"#,
        );
        let all = store.list_instances().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "fedora43");
        assert_eq!(all[1].name, "php-8.5");
        assert_eq!(store.resolve_instance("php-8.5").unwrap().owner, "/tmp/a");
        assert_eq!(
            store.resolve_instance("phprt-fedora43").unwrap().image_ref,
            "quay.io/fedora/fedora:43"
        );
        assert!(store.resolve_instance("missing").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_image_refuses_when_an_instance_uses_it() {
        let dir = std::env::temp_dir().join(format!("phprt-bi-rm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::at(dir.clone()).unwrap();
        let digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        std::fs::create_dir_all(store.image_dir(digest).join("rootfs/usr")).unwrap();
        store
            .record(
                digest,
                "quay.io/fedora/fedora:43",
                None,
                "linux/amd64",
                "",
                1,
            )
            .unwrap();
        write_meta(
            &store,
            "phprt-fedora43",
            r#"{
              "id": "phprt-fedora43",
              "name": "fedora43",
              "image": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
              "owner": "/tmp/a",
              "created": "now"
            }"#,
        );
        let image = store.resolve("aaaaaaaaaaaa").unwrap();
        let err = store.remove_image(&image).unwrap_err().to_string();
        assert!(err.contains("仍被以下实例使用"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }
}
