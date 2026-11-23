//! OCI 构建环境：镜像缓存、rootfs 解包、libcontainer 实例和容器内执行。
//!
//! 本 crate 不依赖 phprt 的 recipe / bundle / state 业务模块；调用方负责把
//! 项目路径、挂载集合和命令传进来。

mod pull;
mod runtime;
mod store;
mod types;
mod unpack;

use std::path::Path;

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

pub use runtime::{
    InstanceLock, delete_instance, ensure_instance, exec, is_running, lock_instance,
    try_lock_instance,
};
pub use store::{Store, digest_hex, dir_size, normalize_digest, short_digest};
pub use types::{ImageInfo, InstanceInfo, InstanceRequest, MountSpec};
pub use unpack::detect_pm;

impl Store {
    pub fn add_image(
        &self,
        reference: &str,
        alias: Option<&str>,
        offline: bool,
    ) -> Result<ImageInfo> {
        pull::add(self, reference, alias, offline)
    }
}

/// 项目根路径的稳定短 hash。默认实例名用它避免不同项目意外共享。
pub fn project_hash(root: &Path) -> String {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let bytes = hasher.finalize();
    let mut out = String::with_capacity(8);
    for byte in bytes.iter().take(4) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 构建实例 id。实例是用户级全局对象，id 只由名字决定。
pub fn instance_id(name: &str) -> String {
    format!("phprt-{name}")
}

/// 实例名会进 id 和 systemd scope，限制成安全字符。
pub fn validate_instance_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("实例名不能为空");
    }
    if name.len() > 64 {
        bail!("实例名太长（最多 64 个字符）：{name}");
    }
    let mut chars = name.chars();
    let first = chars.next().expect("非空已检查");
    if !first.is_ascii_alphanumeric() {
        bail!("实例名必须以字母或数字开头：{name}");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        bail!("实例名只能包含字母、数字、点、下划线和连字符：{name}");
    }
    Ok(())
}

/// 从旧实例 id 推导用户可见名字。
///
/// 旧格式是 `phprt-<项目hash8>-<产品>-<系列>-<digest12>`；新格式是
/// `phprt-<name>`，不会命中这里。
pub fn legacy_instance_name(id: &str) -> Option<String> {
    let rest = id.strip_prefix("phprt-")?;
    let parts: Vec<&str> = rest.split('-').collect();
    if parts.len() < 4 {
        return None;
    }
    let hash = parts[0];
    let digest = parts[parts.len() - 1];
    let is_hex = |value: &str, len: usize| {
        value.len() == len && value.chars().all(|c| c.is_ascii_hexdigit())
    };
    if !is_hex(hash, 8) || !is_hex(digest, 12) {
        return None;
    }
    Some(parts[1..parts.len() - 1].join("-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_helpers() {
        let digest = "sha256:c9fbabbeee0eed88dbeb4ff21d29f7d38abadf07d512016ab8eb17a41c5ac167";
        assert_eq!(short_digest(digest), "c9fbabbeee0e");
        assert_eq!(digest_hex(digest), &digest[7..]);
        assert_eq!(normalize_digest(&digest[7..]).as_deref(), Some(digest));
        assert_eq!(normalize_digest("fedora43"), None);
    }

    #[test]
    fn instance_ids_are_global_and_named() {
        assert_eq!(instance_id("php-8.5"), "phprt-php-8.5");
        assert!(validate_instance_name("php-8.5").is_ok());
        assert!(validate_instance_name("fedora43mybuild_os").is_ok());
        assert!(validate_instance_name("").is_err());
        assert!(validate_instance_name("-php").is_err());
        assert!(validate_instance_name("php/8.5").is_err());
    }

    #[test]
    fn legacy_instance_names_are_derived() {
        assert_eq!(
            legacy_instance_name("phprt-4add57aa-php-8.5-bfc36aecdece").as_deref(),
            Some("php-8.5")
        );
        assert_eq!(legacy_instance_name("phprt-php-8.5"), None);
        assert_eq!(legacy_instance_name("phprt-my-prod-1.0-4add57aa"), None);
        assert_eq!(legacy_instance_name("phprt-fedora43mybuild_os"), None);
    }

    #[test]
    fn store_add_and_resolve_alias() {
        let dir = std::env::temp_dir().join(format!("phprt-bi-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::at(dir.clone()).unwrap();
        let digest = "sha256:c9fbabbeee0eed88dbeb4ff21d29f7d38abadf07d512016ab8eb17a41c5ac167";
        let image_dir = store.image_dir(digest);
        std::fs::create_dir_all(image_dir.join("rootfs/usr")).unwrap();
        store
            .record(
                digest,
                "quay.io/fedora/fedora:43",
                Some("fedora43"),
                "linux/amd64",
                "2026-11-23T00:00:00Z",
                10,
            )
            .unwrap();
        let image = store.resolve("fedora43").unwrap();
        assert_eq!(image.digest, digest);
        assert_eq!(store.resolve(digest).unwrap().digest, digest);
        assert_eq!(store.resolve("c9fbabbeee0e").unwrap().digest, digest);
        assert_eq!(
            store.resolve("quay.io/fedora/fedora:43").unwrap().digest,
            digest
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn newer_ref_replaces_older_ref() {
        let dir = std::env::temp_dir().join(format!("phprt-bi-ref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::at(dir.clone()).unwrap();
        let old = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let new = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        for digest in [old, new] {
            std::fs::create_dir_all(store.image_dir(digest).join("rootfs/usr")).unwrap();
        }
        store
            .record(old, "quay.io/fedora/fedora:43", None, "linux/amd64", "", 1)
            .unwrap();
        store
            .record(new, "quay.io/fedora/fedora:43", None, "linux/amd64", "", 1)
            .unwrap();
        assert_eq!(
            store.resolve("quay.io/fedora/fedora:43").unwrap().digest,
            new
        );
        assert_eq!(store.resolve(old).unwrap().digest, old);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
