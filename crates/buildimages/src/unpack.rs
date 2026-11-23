use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};

use crate::store::write_atomic;

/// 按顺序应用所有层。
pub(crate) struct LayerFile {
    pub media_type: String,
    pub digest: String,
    pub path: PathBuf,
}

/// 解包时临时放宽、复制到实例后要恢复的 mode。
///
/// 旧版本只把普通文件的 mode 写成裸 map；读取时兼容这种格式。
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct Modes {
    #[serde(default)]
    pub files: BTreeMap<String, u32>,
    #[serde(default)]
    pub dirs: BTreeMap<String, u32>,
}

enum Whiteout {
    Remove(PathBuf),
    Opaque(PathBuf),
}

pub(crate) fn unpack(dir: &Path, layers: &[LayerFile]) -> Result<()> {
    let rootfs = dir.join("rootfs");
    let tmp = dir.join("rootfs.tmp");
    if tmp.exists() {
        make_removable(&tmp)?;
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;
    let mut dirs = BTreeMap::new();
    for layer in layers {
        apply_layer(&layer.path, &layer.media_type, &tmp, &mut dirs)
            .with_context(|| format!("解包层 {} 失败", layer.digest))?;
    }
    let files = prepare_for_copy(&tmp)?;
    write_atomic(
        &dir.join("modes.json"),
        &serde_json::to_vec(&Modes { files, dirs })?,
    )?;
    if rootfs.exists() {
        make_removable(&rootfs)?;
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

fn open_archive(path: &Path, media: &str) -> Result<tar::Archive<Box<dyn Read>>> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = if is_gzip(path, media)? {
        Box::new(GzDecoder::new(BufReader::new(file)))
    } else {
        Box::new(BufReader::new(file))
    };
    Ok(tar::Archive::new(reader))
}

fn apply_layer(
    path: &Path,
    media: &str,
    dst: &Path,
    dirs: &mut BTreeMap<String, u32>,
) -> Result<()> {
    let dst_canon = fs::canonicalize(dst).with_context(|| format!("读不到 {}", dst.display()))?;

    // 第一遍只读：whiteout 只作用于下层，必须在解包本层普通条目前应用。
    for whiteout in collect_whiteouts(path, media)? {
        apply_whiteout(dst, &dst_canon, whiteout, dirs)?;
    }

    let mut archive = open_archive(path, media)?;
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
        if is_whiteout(name) {
            continue;
        }
        let entry_type = entry.header().entry_type();
        if entry_type.is_character_special() || entry_type.is_block_special() {
            continue;
        }
        if let Some(parent) = rel.parent() {
            ensure_writable_dirs(dst, &dst_canon, parent, dirs)?;
        }
        let target = dst.join(&rel);
        replace_incompatible(&dst_canon, &target, &rel, entry_type.is_dir(), dirs)?;
        entry
            .unpack_in(dst)
            .with_context(|| format!("写入 {}", rel.display()))?;
        if entry_type.is_dir() {
            record_dir_mode(&dst_canon, &rel, dirs)?;
        }
    }
    Ok(())
}

fn collect_whiteouts(path: &Path, media: &str) -> Result<Vec<Whiteout>> {
    let mut out = Vec::new();
    let mut archive = open_archive(path, media)?;
    for entry in archive.entries()? {
        let entry = entry?;
        let raw_path = entry.path()?.into_owned();
        let Some(rel) = normalize_entry_path(&raw_path)? else {
            continue;
        };
        let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let parent = rel.parent().unwrap_or(Path::new("")).to_path_buf();
        if name == ".wh..wh..opq" {
            out.push(Whiteout::Opaque(parent));
        } else if let Some(target) = name.strip_prefix(".wh.") {
            check_whiteout_target(target, &raw_path)?;
            out.push(Whiteout::Remove(parent.join(target)));
        }
    }
    Ok(out)
}

fn check_whiteout_target(target: &str, raw: &Path) -> Result<()> {
    if target.is_empty() || target == "." || target == ".." || target.contains('/') {
        bail!(
            "层里有非法 whiteout {}：目标名不能为空、`.`、`..` 或包含 `/`",
            raw.display()
        );
    }
    Ok(())
}

fn is_whiteout(name: &str) -> bool {
    name == ".wh..wh..opq" || name.starts_with(".wh.")
}

fn apply_whiteout(
    dst: &Path,
    dst_canon: &Path,
    whiteout: Whiteout,
    dirs: &mut BTreeMap<String, u32>,
) -> Result<()> {
    match whiteout {
        Whiteout::Remove(rel) => {
            let parent = rel.parent().unwrap_or(Path::new(""));
            ensure_writable_dirs(dst, dst_canon, parent, dirs)?;
            let key = resolved_rel(dst, dst_canon, &rel);
            remove_path(&dst.join(&rel))?;
            match key {
                Some(key) => purge_dir_modes(dirs, &key, true),
                None => {
                    let raw = rel.to_string_lossy();
                    purge_dir_modes(dirs, &raw, true);
                }
            }
            Ok(())
        }
        Whiteout::Opaque(rel) => {
            let dir = dst.join(&rel);
            if !dir.exists() {
                return Ok(());
            }
            let Some(dir_canon) = ensure_inside(dst_canon, &dir)? else {
                return Ok(());
            };
            ensure_writable_dirs(dst, dst_canon, &rel, dirs)?;
            if !dir.is_dir() {
                return Ok(());
            }
            for child in fs::read_dir(&dir)? {
                remove_path(&child?.path())?;
            }
            let key = dir_canon
                .strip_prefix(dst_canon)
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .to_string();
            purge_dir_modes(dirs, &key, false);
            Ok(())
        }
    }
}

fn normalize_entry_path(path: &Path) -> Result<Option<PathBuf>> {
    let mut out = PathBuf::new();
    for comp in path.components() {
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

/// 返回 path 的 canonical 路径；不存在时返回 None。
/// 如果通过符号链接跑到 dst 之外，直接报错。
fn ensure_inside(dst_canon: &Path, path: &Path) -> Result<Option<PathBuf>> {
    match fs::canonicalize(path) {
        Ok(canon) => {
            if !canon.starts_with(dst_canon) {
                bail!(
                    "层里的路径通过符号链接逃出了 rootfs：{} -> {}",
                    path.display(),
                    canon.display()
                );
            }
            Ok(Some(canon))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("解析 {}", path.display())),
    }
}

/// 普通用户解包时，0555 等目录无法继续写入；先临时补 0700，
/// 原始 mode 记进 dirs，复制到实例后再恢复。
fn ensure_writable_dirs(
    dst: &Path,
    dst_canon: &Path,
    rel: &Path,
    dirs: &mut BTreeMap<String, u32>,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut cur = dst.to_path_buf();
    for comp in rel.components() {
        let Component::Normal(c) = comp else {
            continue;
        };
        cur.push(c);
        let Ok(meta) = fs::metadata(&cur) else {
            continue;
        };
        let Some(canon) = ensure_inside(dst_canon, &cur)? else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let mode = meta.permissions().mode() & 0o7777;
        if mode & 0o700 == 0o700 {
            continue;
        }
        let key = canon
            .strip_prefix(dst_canon)
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .to_string();
        if key.is_empty() {
            continue;
        }
        dirs.entry(key).or_insert(mode);
        let mut perms = meta.permissions();
        perms.set_mode(mode | 0o700);
        fs::set_permissions(&canon, perms)
            .with_context(|| format!("调整目录权限 {}", canon.display()))?;
    }
    Ok(())
}

/// 目录条目 unpack 后记录最终 mode；如果最终 mode 本来就带 0700，
/// 说明不需要恢复，清掉之前层留下的记录。
fn record_dir_mode(dst_canon: &Path, rel: &Path, dirs: &mut BTreeMap<String, u32>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let target = dst_canon.join(rel);
    let Ok(meta) = fs::symlink_metadata(&target) else {
        return Ok(());
    };
    if !meta.is_dir() {
        return Ok(());
    }
    let Some(canon) = ensure_inside(dst_canon, &target)? else {
        return Ok(());
    };
    let Ok(meta) = fs::symlink_metadata(&canon) else {
        return Ok(());
    };
    if !meta.is_dir() {
        return Ok(());
    }
    let key = canon
        .strip_prefix(dst_canon)
        .unwrap_or(Path::new(""))
        .to_string_lossy()
        .to_string();
    if key.is_empty() {
        return Ok(());
    }
    let mode = meta.permissions().mode() & 0o7777;
    if mode & 0o700 == 0o700 {
        dirs.remove(&key);
        return Ok(());
    }
    dirs.insert(key, mode);
    let mut perms = meta.permissions();
    perms.set_mode(mode | 0o700);
    fs::set_permissions(&canon, perms)
        .with_context(|| format!("调整目录权限 {}", canon.display()))?;
    Ok(())
}

/// tar-rs 只能覆盖文件/符号链接；文件与目录互换要求先删旧路径。
fn replace_incompatible(
    dst_canon: &Path,
    target: &Path,
    rel: &Path,
    entry_is_dir: bool,
    dirs: &mut BTreeMap<String, u32>,
) -> Result<()> {
    let Ok(meta) = fs::symlink_metadata(target) else {
        return Ok(());
    };
    if entry_is_dir == meta.is_dir() {
        return Ok(());
    }
    let key = if meta.is_dir() {
        fs::canonicalize(target).ok().and_then(|canon| {
            canon
                .strip_prefix(dst_canon)
                .ok()
                .map(|path| path.to_string_lossy().to_string())
        })
    } else {
        None
    };
    remove_path(target)?;
    if meta.is_dir() {
        match key {
            Some(key) => purge_dir_modes(dirs, &key, true),
            None => {
                let raw = rel.to_string_lossy();
                purge_dir_modes(dirs, &raw, true);
            }
        }
    }
    Ok(())
}

/// 条目路径按中间符号链接解析后的相对路径，用于清理目录 mode 记录。
fn resolved_rel(dst: &Path, dst_canon: &Path, rel: &Path) -> Option<String> {
    let name = rel.file_name()?;
    let parent = rel.parent().unwrap_or(Path::new(""));
    let parent_canon = fs::canonicalize(dst.join(parent)).ok()?;
    if !parent_canon.starts_with(dst_canon) {
        return None;
    }
    parent_canon
        .join(name)
        .strip_prefix(dst_canon)
        .ok()
        .map(|path| path.to_string_lossy().to_string())
}

/// 删掉某个目录后，之前记录的目录 mode 不能再套到新出现的同名目录上。
fn purge_dir_modes(dirs: &mut BTreeMap<String, u32>, prefix: &str, include_self: bool) {
    if prefix.is_empty() {
        if !include_self {
            dirs.clear();
        }
        return;
    }
    let child_prefix = format!("{prefix}/");
    dirs.retain(|key, _| !(key.starts_with(&child_prefix) || include_self && key == prefix));
}

fn remove_path(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {
            make_removable(path)?;
            fs::remove_dir_all(path)?;
        }
        Ok(_) => fs::remove_file(path)?,
        Err(_) => {}
    }
    Ok(())
}

/// 递归把目录补成 owner 可写，方便宿主普通用户删除实例或旧 rootfs。
pub(crate) fn make_removable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if !meta.is_dir() {
        return Ok(());
    }
    let mode = meta.permissions().mode();
    if mode & 0o700 != 0o700 {
        let mut perms = meta.permissions();
        perms.set_mode(mode | 0o700);
        fs::set_permissions(path, perms)
            .with_context(|| format!("调整目录权限 {}", path.display()))?;
    }
    for entry in fs::read_dir(path)? {
        let child = entry?.path();
        if let Ok(meta) = fs::symlink_metadata(&child)
            && meta.is_dir()
        {
            make_removable(&child)?;
        }
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

fn read_modes(bytes: &[u8]) -> Result<Modes> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let current = value.get("files").is_some_and(|item| item.is_object())
        || value.get("dirs").is_some_and(|item| item.is_object());
    if current {
        Ok(serde_json::from_value(value)?)
    } else {
        // 旧格式：只记录普通文件的裸 map。
        Ok(Modes {
            files: serde_json::from_value(value)?,
            dirs: BTreeMap::new(),
        })
    }
}

/// 复制完成后恢复解包时改过的 mode（setuid、目录 0555 等）。
pub(crate) fn restore_modes(image_dir: &Path, rootfs: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let path = image_dir.join("modes.json");
    if !path.exists() {
        return Ok(());
    }
    let modes = read_modes(&fs::read(&path)?)?;
    for (rel, mode) in modes.files {
        let target = rootfs.join(&rel);
        let Ok(meta) = fs::symlink_metadata(&target) else {
            continue;
        };
        if !meta.file_type().is_file() {
            continue;
        }
        let mut perms = meta.permissions();
        perms.set_mode(mode);
        fs::set_permissions(&target, perms)?;
    }
    // 先恢复深层目录，避免父目录先变成 0400 后无法解析子路径。
    let mut dirs: Vec<(String, u32)> = modes.dirs.into_iter().collect();
    dirs.sort_by_key(|(rel, _)| std::cmp::Reverse(rel.matches('/').count()));
    for (rel, mode) in dirs {
        let target = rootfs.join(&rel);
        let Ok(meta) = fs::symlink_metadata(&target) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let mut perms = meta.permissions();
        perms.set_mode(mode);
        fs::set_permissions(&target, perms)?;
    }
    Ok(())
}

/// 镜像里有没有可用的 shell 和包管理器。返回包管理器名（dnf / apt）。
pub fn detect_pm(rootfs: &Path) -> Result<&'static str> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::os::unix::fs::PermissionsExt;
    use tar::{Builder, EntryType, Header};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("phprt-unpack-{name}-{}", std::process::id()));
        let _ = make_removable(&dir);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = make_removable(dir);
        let _ = fs::remove_dir_all(dir);
    }

    fn layer(dir: &Path, name: &str, build: impl FnOnce(&mut Builder<File>)) -> LayerFile {
        let path = dir.join(name);
        let file = File::create(&path).unwrap();
        let mut builder = Builder::new(file);
        build(&mut builder);
        builder.finish().unwrap();
        LayerFile {
            media_type: "application/vnd.oci.image.layer.v1.tar".to_string(),
            digest: name.to_string(),
            path,
        }
    }

    fn add_file(builder: &mut Builder<File>, path: &str, mode: u32, data: &[u8]) {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        builder.append_data(&mut header, path, data).unwrap();
    }

    fn add_dir(builder: &mut Builder<File>, path: &str, mode: u32) {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Directory);
        header.set_size(0);
        header.set_mode(mode);
        header.set_cksum();
        builder.append_data(&mut header, path, &[][..]).unwrap();
    }

    fn add_symlink(builder: &mut Builder<File>, path: &str, target: &str) {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_link_name(target).unwrap();
        header.set_cksum();
        builder.append_data(&mut header, path, &[][..]).unwrap();
    }

    fn mode_of(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
    }

    #[test]
    fn opaque_whiteout_applies_before_same_layer_files() {
        let root = temp("opq");
        let first = layer(&root, "l1.tar", |builder| {
            add_dir(builder, "a/", 0o755);
            add_file(builder, "a/old.conf", 0o644, b"old");
        });
        let second = layer(&root, "l2.tar", |builder| {
            add_dir(builder, "a/", 0o755);
            add_file(builder, "a/new.conf", 0o644, b"new");
            add_file(builder, "a/.wh..wh..opq", 0o644, b"");
        });
        unpack(&root, &[first, second]).unwrap();
        assert!(root.join("rootfs/a/new.conf").is_file());
        assert!(!root.join("rootfs/a/old.conf").exists());
        cleanup(&root);
    }

    #[test]
    fn explicit_whiteout_does_not_hide_same_layer_file() {
        let root = temp("wh");
        let first = layer(&root, "l1.tar", |builder| {
            add_dir(builder, "a/", 0o755);
            add_file(builder, "a/foo", 0o644, b"lower");
        });
        let second = layer(&root, "l2.tar", |builder| {
            add_dir(builder, "a/", 0o755);
            add_file(builder, "a/foo", 0o644, b"upper");
            add_file(builder, "a/.wh.foo", 0o644, b"");
        });
        unpack(&root, &[first, second]).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("rootfs/a/foo")).unwrap(),
            "upper"
        );
        cleanup(&root);
    }

    #[test]
    fn invalid_whiteouts_are_rejected_without_deleting() {
        for (index, name) in [".wh.", ".wh..", ".wh..."].iter().enumerate() {
            let root = temp(&format!("badwh-{index}"));
            let first = layer(&root, "l1.tar", |builder| {
                add_dir(builder, "a/", 0o755);
                add_file(builder, "a/victim", 0o644, b"keep");
            });
            let second = layer(&root, "l2.tar", |builder| {
                add_file(builder, &format!("a/{name}"), 0o644, b"");
            });
            let err = unpack(&root, &[first, second]).unwrap_err();
            assert!(
                format!("{err:#}").contains("非法 whiteout"),
                "unexpected error: {err:#}"
            );
            assert!(
                root.join("rootfs.tmp/a/victim").is_file(),
                "victim was deleted by {name}"
            );
            cleanup(&root);
        }
    }

    #[test]
    fn whiteout_cannot_escape_through_symlink() {
        let root = temp("escape");
        let outside = root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("victim"), b"keep").unwrap();
        let target = fs::canonicalize(&outside).unwrap();
        let first = layer(&root, "l1.tar", |builder| {
            add_symlink(builder, "lnk", target.to_str().unwrap());
        });
        let second = layer(&root, "l2.tar", |builder| {
            add_file(builder, "lnk/.wh.victim", 0o644, b"");
        });
        let err = unpack(&root, &[first, second]).unwrap_err();
        assert!(
            format!("{err:#}").contains("逃出了 rootfs"),
            "unexpected error: {err:#}"
        );
        assert!(outside.join("victim").is_file());
        cleanup(&root);
    }

    #[test]
    fn directory_modes_are_recorded_and_restored() {
        let root = temp("dirmodes");
        let layer_file = layer(&root, "l1.tar", |builder| {
            add_dir(builder, "ro/", 0o555);
            add_file(builder, "ro/file", 0o644, b"x");
            add_dir(builder, "rw/", 0o755);
            add_file(builder, "secret", 0o000, b"s");
        });
        unpack(&root, &[layer_file]).unwrap();
        let rootfs = root.join("rootfs");
        assert_eq!(mode_of(&rootfs.join("ro")), 0o755);
        assert_eq!(mode_of(&rootfs.join("secret")), 0o400);

        let modes = read_modes(&fs::read(root.join("modes.json")).unwrap()).unwrap();
        assert_eq!(modes.dirs.get("ro").copied(), Some(0o555));
        assert!(!modes.dirs.contains_key("rw"));
        assert_eq!(modes.files.get("secret").copied(), Some(0o000));

        restore_modes(&root, &rootfs).unwrap();
        assert_eq!(mode_of(&rootfs.join("ro")), 0o555);
        assert_eq!(mode_of(&rootfs.join("secret")), 0o000);
        cleanup(&root);
    }

    #[test]
    fn normal_entry_cannot_escape_through_symlink() {
        let root = temp("escape-normal");
        let outside = root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("victim"), b"keep").unwrap();
        let target = fs::canonicalize(&outside).unwrap();
        let first = layer(&root, "l1.tar", |builder| {
            add_symlink(builder, "lnk", target.to_str().unwrap());
        });
        let second = layer(&root, "l2.tar", |builder| {
            add_file(builder, "lnk/evil", 0o644, b"bad");
        });
        let err = unpack(&root, &[first, second]).unwrap_err();
        assert!(
            format!("{err:#}").contains("逃出了 rootfs"),
            "unexpected error: {err:#}"
        );
        assert!(outside.join("victim").is_file());
        assert!(!outside.join("evil").exists());
        cleanup(&root);
    }

    #[test]
    fn legacy_modes_file_is_read() {
        let modes = read_modes(br#"{"secret":0}"#).unwrap();
        assert_eq!(modes.files.get("secret"), Some(&0));
        assert!(modes.dirs.is_empty());
    }

    #[test]
    fn restore_skips_symlink_file_paths() {
        let root = temp("modes-symlink");
        let image = root.join("image");
        let rootfs = image.join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();
        let outside = root.join("outside");
        fs::write(&outside, b"x").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&outside, rootfs.join("s")).unwrap();
        fs::write(image.join("modes.json"), br#"{"files":{"s":0},"dirs":{}}"#).unwrap();
        restore_modes(&image, &rootfs).unwrap();
        assert_eq!(mode_of(&outside), 0o640);
        cleanup(&root);
    }

    #[test]
    fn removed_directory_modes_are_not_reused_for_implicit_dirs() {
        let root = temp("purge-dir-modes");
        let first = layer(&root, "l1.tar", |builder| {
            add_dir(builder, "a/", 0o555);
            add_dir(builder, "a/b/", 0o555);
            add_file(builder, "a/b/old", 0o644, b"old");
        });
        let second = layer(&root, "l2.tar", |builder| {
            add_file(builder, "a/.wh.b", 0o644, b"");
            add_file(builder, "a/b/new", 0o644, b"new");
        });
        unpack(&root, &[first, second]).unwrap();
        let modes = read_modes(&fs::read(root.join("modes.json")).unwrap()).unwrap();
        assert_eq!(modes.dirs.get("a").copied(), Some(0o555));
        assert!(!modes.dirs.contains_key("a/b"));
        assert_eq!(
            fs::read_to_string(root.join("rootfs/a/b/new")).unwrap(),
            "new"
        );
        cleanup(&root);
    }

    #[test]
    fn file_and_directory_type_replacement() {
        let root = temp("replace-dir-to-file");
        let first = layer(&root, "l1.tar", |builder| {
            add_dir(builder, "x/", 0o555);
            add_file(builder, "x/inner", 0o644, b"old");
        });
        let second = layer(&root, "l2.tar", |builder| {
            add_file(builder, "x", 0o644, b"now a file");
        });
        unpack(&root, &[first, second]).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("rootfs/x")).unwrap(),
            "now a file"
        );
        cleanup(&root);

        let root = temp("replace-file-to-dir");
        let first = layer(&root, "l1.tar", |builder| {
            add_file(builder, "y", 0o644, b"old file");
        });
        let second = layer(&root, "l2.tar", |builder| {
            add_dir(builder, "y/", 0o755);
            add_file(builder, "y/inner", 0o644, b"new");
        });
        unpack(&root, &[first, second]).unwrap();
        assert!(root.join("rootfs/y").is_dir());
        assert_eq!(
            fs::read_to_string(root.join("rootfs/y/inner")).unwrap(),
            "new"
        );
        cleanup(&root);
    }
}
