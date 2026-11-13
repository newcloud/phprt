use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 在路径后面追加后缀，例如 dest.part / dest.part.2 / dest.part.json。
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(suffix);
    PathBuf::from(raw)
}

pub fn part_path(dest: &Path) -> PathBuf {
    with_suffix(dest, ".part")
}

pub fn chunk_path(dest: &Path, index: usize) -> PathBuf {
    with_suffix(dest, &format!(".part.{index}"))
}

pub fn meta_path(dest: &Path) -> PathBuf {
    with_suffix(dest, ".part.json")
}

pub fn size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// 分片续传的元数据。校验值变化时整包重下，避免拼接出坏文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartMeta {
    pub url: String,
    #[serde(default)]
    pub etag: String,
    #[serde(default)]
    pub last_modified: String,
    pub total: u64,
    pub chunks: usize,
}

pub fn read_meta(path: &Path) -> Option<PartMeta> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_meta(path: &Path, meta: &PartMeta) -> Result<()> {
    let tmp = with_suffix(path, ".tmp");
    fs::write(&tmp, serde_json::to_vec(meta)?)
        .with_context(|| format!("写不到 {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("写不到 {}", path.display()))?;
    Ok(())
}

/// 删除这个目标的所有下载中间文件。
pub fn cleanup(dest: &Path) {
    let _ = fs::remove_file(part_path(dest));
    let _ = fs::remove_file(meta_path(dest));

    let (Some(dir), Some(name)) = (dest.parent(), dest.file_name().and_then(|s| s.to_str())) else {
        return;
    };
    let prefix = format!("{name}.part.");
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(file) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if file.starts_with(&prefix) && file != format!("{name}.part.json") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("读不到 {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_removes_parts_but_keeps_the_file() {
        let dir = std::env::temp_dir().join(format!("phprt-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("a.tar.gz");
        fs::write(&dest, b"final").unwrap();
        fs::write(part_path(&dest), b"partial").unwrap();
        fs::write(chunk_path(&dest, 0), b"chunk").unwrap();
        fs::write(chunk_path(&dest, 3), b"chunk").unwrap();
        write_meta(
            &meta_path(&dest),
            &PartMeta {
                url: "u".into(),
                etag: String::new(),
                last_modified: String::new(),
                total: 3,
                chunks: 2,
            },
        )
        .unwrap();

        cleanup(&dest);

        assert!(dest.exists());
        assert!(!part_path(&dest).exists());
        assert!(!chunk_path(&dest, 0).exists());
        assert!(!chunk_path(&dest, 3).exists());
        assert!(!meta_path(&dest).exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
