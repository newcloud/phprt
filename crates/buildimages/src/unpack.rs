use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;

use crate::store::write_atomic;

/// 按顺序应用所有层。
pub(crate) struct LayerFile {
    pub media_type: String,
    pub digest: String,
    pub path: PathBuf,
}

pub(crate) fn unpack(dir: &Path, layers: &[LayerFile]) -> Result<()> {
    let rootfs = dir.join("rootfs");
    let tmp = dir.join("rootfs.tmp");
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;
    for layer in layers {
        apply_layer(&layer.path, &layer.media_type, &tmp)
            .with_context(|| format!("解包层 {} 失败", layer.digest))?;
    }
    let modes = prepare_for_copy(&tmp)?;
    write_atomic(&dir.join("modes.json"), &serde_json::to_vec(&modes)?)?;
    if rootfs.exists() {
        fs::remove_dir_all(&rootfs)?;
    }
    fs::rename(&tmp, &rootfs)?;
    Ok(())
}

fn is_gzip(path: &Path, media: &str) -> Result<bool> {
    if media.contains("gzip") {
        return Ok(true);
    }
    if media.contains("zstd") {
        bail!("层是 zstd 压缩；当前只支持 gzip 和未压缩 tar");
    }
    let mut file = File::open(path)?;
    let mut magic = [0u8; 2];
    let n = file.read(&mut magic)?;
    Ok(n == 2 && magic == [0x1f, 0x8b])
}

fn apply_layer(path: &Path, media: &str, dst: &Path) -> Result<()> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = if is_gzip(path, media)? {
        Box::new(GzDecoder::new(BufReader::new(file)))
    } else {
        Box::new(BufReader::new(file))
    };
    let mut archive = tar::Archive::new(reader);
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);
    archive.set_unpack_xattrs(false);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.into_owned();
        let Some(rel) = normalize_entry_path(&raw_path)? else {
            continue;
        };
        let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let entry_type = entry.header().entry_type();
        if name == ".wh..wh..opq" {
            let dir = dst.join(rel.parent().unwrap_or(Path::new("")));
            ensure_writable_dirs(dst, rel.parent().unwrap_or(Path::new("")))?;
            if dir.is_dir() {
                for child in fs::read_dir(&dir)? {
                    remove_path(&child?.path())?;
                }
            }
            continue;
        }
        if let Some(target) = name.strip_prefix(".wh.") {
            ensure_writable_dirs(dst, rel.parent().unwrap_or(Path::new("")))?;
            remove_path(&dst.join(rel.parent().unwrap_or(Path::new(""))).join(target))?;
            continue;
        }
        if entry_type.is_character_special() || entry_type.is_block_special() {
            continue;
        }
        if let Some(parent) = rel.parent() {
            ensure_writable_dirs(dst, parent)?;
        }
        entry
            .unpack_in(dst)
            .with_context(|| format!("写入 {}", rel.display()))?;
        if entry_type.is_dir()
            && let Ok(meta) = fs::metadata(dst.join(&rel))
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o700);
            let _ = fs::set_permissions(dst.join(&rel), perms);
        }
    }
    Ok(())
}

fn normalize_entry_path(path: &Path) -> Result<Option<PathBuf>> {
    let mut out = PathBuf::new();
    for comp in path.components() {
        use std::path::Component;
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::RootDir | Component::ParentDir | Component::Prefix(_) => {
                bail!("层里有不安全的路径：{}", path.display())
            }
        }
    }
    if out.as_os_str().is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

fn ensure_writable_dirs(dst: &Path, rel: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut cur = dst.to_path_buf();
    for comp in rel.components() {
        if let std::path::Component::Normal(c) = comp {
            cur.push(c);
            if let Ok(meta) = fs::metadata(&cur) {
                let mut perms = meta.permissions();
                let mode = perms.mode();
                if mode & 0o700 != 0o700 {
                    perms.set_mode(mode | 0o700);
                    fs::set_permissions(&cur, perms)?;
                }
            }
        }
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path)?,
        Ok(_) => fs::remove_file(path)?,
        Err(_) => {}
    }
    Ok(())
}

/// 镜像里有 `--x--x--x` / `r-s--x--x` 这类普通用户读不了的文件。
/// 解包后补 owner-read 位，记录原始 mode，复制到实例后再恢复。
fn prepare_for_copy(rootfs: &Path) -> Result<BTreeMap<String, u32>> {
    use std::os::unix::fs::PermissionsExt;
    let mut modes = BTreeMap::new();
    fn walk(root: &Path, dir: &Path, modes: &mut BTreeMap<String, u32>) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                walk(root, &path, modes)?;
                continue;
            }
            let mode = meta.permissions().mode() & 0o7777;
            if mode & 0o400 == 0 {
                let rel = path.strip_prefix(root)?.to_string_lossy().to_string();
                modes.insert(rel, mode);
                let mut perms = meta.permissions();
                perms.set_mode(mode | 0o400);
                fs::set_permissions(&path, perms)?;
            }
        }
        Ok(())
    }
    walk(rootfs, rootfs, &mut modes)?;
    Ok(modes)
}

/// 复制完成后恢复解包时改过的 mode（主要是 setuid）。
pub(crate) fn restore_modes(image_dir: &Path, rootfs: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let path = image_dir.join("modes.json");
    if !path.exists() {
        return Ok(());
    }
    let modes: BTreeMap<String, u32> = serde_json::from_slice(&fs::read(&path)?)?;
    for (rel, mode) in modes {
        let target = rootfs.join(&rel);
        if fs::symlink_metadata(&target).is_err() {
            continue;
        }
        let mut perms = fs::symlink_metadata(&target)?.permissions();
        perms.set_mode(mode);
        fs::set_permissions(&target, perms)?;
    }
    Ok(())
}

/// 镜像里有没有可用的 shell 和包管理器。返回包管理器名（dnf / apt）。
pub(crate) fn detect_pm(rootfs: &Path) -> Result<&'static str> {
    let has = |path: &str| rootfs.join(path).exists();
    if !has("bin/sh") && !has("usr/bin/sh") {
        bail!("镜像里没有 /bin/sh，不能作为构建环境");
    }
    if has("usr/bin/dnf") || has("bin/dnf") {
        return Ok("dnf");
    }
    if has("usr/bin/apt-get") || has("bin/apt-get") {
        return Ok("apt");
    }
    bail!("镜像里没有 dnf 或 apt-get，当前只支持 Fedora / Debian 系列")
}
