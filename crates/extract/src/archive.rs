use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::Path;

use anyhow::{Context, Result, bail};
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use lzma_rust2::XzReader;
use tar::Archive;
use zip::ZipArchive;

/// 进度总量的口径：
/// - tar.* 用压缩包字节数；
/// - zip 用各文件解压后大小之和。
pub fn expected_total(path: &Path) -> Result<u64> {
    let name = archive_name(path);
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        let mut zip = ZipArchive::new(open(path)?).context("读取 zip 目录失败")?;
        let mut total = 0;
        for index in 0..zip.len() {
            let entry = zip.by_index_raw(index).context("读取 zip 条目失败")?;
            if !entry.is_dir() {
                total += entry.size();
            }
        }
        Ok(total)
    } else if is_tar(&lower) {
        Ok(fs::metadata(path)
            .with_context(|| format!("读不到 {}", path.display()))?
            .len())
    } else {
        Ok(0)
    }
}

/// 按后缀选择解压器；解到 dest 目录里，保留文件权限。
/// `progress` 收到的是已消费的字节数（解压前大小），用于进度条。
pub fn extract<F: FnMut(u64)>(archive: &Path, dest: &Path, progress: &mut F) -> Result<()> {
    let name = archive_name(archive);
    let lower = name.to_ascii_lowercase();
    fs::create_dir_all(dest).with_context(|| format!("建不了 {}", dest.display()))?;

    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        let file = counted(open(archive)?, progress);
        untar(GzDecoder::new(file), dest)
    } else if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        let file = counted(open(archive)?, progress);
        untar(XzReader::new(file, true), dest)
    } else if lower.ends_with(".tar.bz2") {
        let file = counted(open(archive)?, progress);
        untar(BzDecoder::new(file), dest)
    } else if lower.ends_with(".zip") {
        unzip(archive, dest, progress)
    } else {
        bail!("不认识的压缩格式: {name}（支持 .tar.gz/.tgz/.tar.xz/.txz/.tar.bz2/.zip）")
    }
}

fn archive_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("archive")
        .to_owned()
}

fn is_tar(lower: &str) -> bool {
    lower.ends_with(".tar.gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".txz")
        || lower.ends_with(".tar.bz2")
}

fn open(path: &Path) -> Result<BufReader<File>> {
    let file = File::open(path).with_context(|| format!("读不到 {}", path.display()))?;
    Ok(BufReader::new(file))
}

fn counted<'a, R: Read, F: FnMut(u64)>(inner: R, progress: &'a mut F) -> CountingReader<'a, R, F> {
    CountingReader { inner, progress }
}

struct CountingReader<'a, R, F> {
    inner: R,
    progress: &'a mut F,
}

impl<R: Read, F: FnMut(u64)> Read for CountingReader<'_, R, F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            (self.progress)(n as u64);
        }
        Ok(n)
    }
}

fn untar<R: Read>(reader: R, dest: &Path) -> Result<()> {
    let mut archive = Archive::new(reader);
    let entries = archive.entries().context("读取 tar 目录失败")?;
    for entry in entries {
        let mut entry = entry.context("读取 tar 条目失败")?;
        let unpacked = entry.unpack_in(dest).context("解包 tar 条目失败")?;
        if !unpacked {
            let path = entry
                .path()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "?".to_owned());
            bail!("压缩包里有越界路径: {path}");
        }
    }
    Ok(())
}

fn zip_name(entry: &zip::read::ZipFile<'_, BufReader<File>>) -> String {
    entry
        .name()
        .map(|name| name.into_owned())
        .unwrap_or_else(|_| "?".to_owned())
}

fn unzip<F: FnMut(u64)>(path: &Path, dest: &Path, progress: &mut F) -> Result<()> {
    let mut zip = ZipArchive::new(open(path)?).context("读取 zip 目录失败")?;
    let mut buf = [0u8; 64 * 1024];
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).context("读取 zip 条目失败")?;
        let Some(relative) = entry.enclosed_name() else {
            bail!("压缩包里有越界路径: {}", zip_name(&entry));
        };
        if entry.is_symlink() {
            bail!("zip 里的符号链接暂不支持: {}", zip_name(&entry));
        }
        let out = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out).with_context(|| format!("建不了 {}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).with_context(|| format!("建不了 {}", parent.display()))?;
        }
        let mut file = File::create(&out).with_context(|| format!("写不到 {}", out.display()))?;
        loop {
            let n = entry.read(&mut buf).context("读取 zip 条目失败")?;
            if n == 0 {
                break;
            }
            use std::io::Write as _;
            file.write_all(&buf[..n])?;
            progress(n as u64);
        }
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o7777))?;
        }
    }
    Ok(())
}
