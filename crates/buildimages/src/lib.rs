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

use anyhow::Result;
use sha2::{Digest, Sha256};

pub use store::{digest_hex, dir_size, normalize_digest, short_digest};
pub use types::{ImageInfo, InstanceInfo, InstanceRequest, MountSpec};
pub use unpack::detect_pm;

pub use runtime::{delete_instance, ensure_instance, exec, runtime_state_dir};

pub use store::Store;

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

/// 项目根路径的稳定短 hash，用来隔离不同项目的构建实例。
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

/// 构建实例 id。libcontainer 会拿它当 systemd scope 名，必须是用户级唯一的。
pub fn instance_id(root: &Path, product: &str, series: &str, digest: &str) -> String {
    format!(
        "phprt-{}-{}-{}-{}",
        project_hash(root),
        sanitize(product),
        sanitize(series),
        short_digest(digest)
    )
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
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
    fn instance_ids_are_namespaced() {
        let a = instance_id(Path::new("/tmp/a"), "php", "8.5", "sha256:aa");
        let b = instance_id(Path::new("/tmp/b"), "php", "8.5", "sha256:aa");
        assert_ne!(a, b);
        assert!(a.starts_with("phprt-"));
        assert!(a.contains("-php-8.5-"));
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
