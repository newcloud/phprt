mod archive;

#[cfg(test)]
mod tests;

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result, bail};
use phprt_source::Source;
use phprt_ui::{Mode, Tracker, tag};

/// 按构建顺序逐个解压；目标目录已有内容时跳过。
/// 下载阶段已经保证压缩包存在（`--downloads` 在这里之前就返回了）。
pub fn run(sources: &[Source], mode: Mode) -> Result<()> {
    if sources.is_empty() {
        return Ok(());
    }

    eprintln!();
    eprintln!("解压源码（按构建顺序，{} 个）", sources.len());
    let tags = sources
        .iter()
        .map(|source| tag(&source.label, &source.name, &source.version))
        .collect();
    let ui = Tracker::new(tags, mode);

    let mut done = 0;
    let mut skipped = 0;
    let result = (|| -> Result<()> {
        for (index, source) in sources.iter().enumerate() {
            if already_unpacked(source)? {
                ui.finish(index, "✓", "已解压");
                skipped += 1;
                continue;
            }
            let detail = format!("{} → {}", archive_name(source), source.unpack_display);
            ui.start(index, "解压", Some(&detail));
            extract_one(source, &ui, index)?;
            ui.finish(index, "✓", "解压完成");
            done += 1;
        }
        Ok(())
    })();

    ui.clear();
    result?;
    ui.line(&format!("解压完成: {done} 个，已存在 {skipped} 个"));
    Ok(())
}

fn archive_name(source: &Source) -> String {
    source
        .archive
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_owned()
}

/// 目录非空就算解压完成；空目录删掉重解。
fn already_unpacked(source: &Source) -> Result<bool> {
    match fs::symlink_metadata(&source.unpack) {
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("读不到 {}", source.unpack.display())),
        Ok(meta) if !meta.is_dir() => {
            bail!("解压目标已存在但不是目录: {}", source.unpack.display())
        }
        Ok(_) => {
            let mut entries = fs::read_dir(&source.unpack)
                .with_context(|| format!("读不到 {}", source.unpack.display()))?;
            if entries.next().is_some() {
                return Ok(true);
            }
            let _ = fs::remove_dir(&source.unpack);
            Ok(false)
        }
    }
}

fn extract_one(source: &Source, ui: &Tracker, index: usize) -> Result<()> {
    if !source.archive.is_file() {
        bail!(
            "源码包不在缓存: {}（先运行 phprt install {} --downloads）",
            source.archive.display(),
            source.version
        );
    }
    let parent = source
        .unpack
        .parent()
        .ok_or_else(|| anyhow::anyhow!("解压目录不合法: {}", source.unpack.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("建不了 {}", parent.display()))?;
    let name = source
        .unpack
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("解压目录不合法: {}", source.unpack.display()))?;

    cleanup_stale(parent, name)?;
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);

    let total = archive::expected_total(&source.archive)?;
    ui.set_total(index, (total > 0).then_some(total));
    let mut progress = |delta| ui.advance(index, delta);

    let result = (|| -> Result<()> {
        archive::extract(&source.archive, &tmp, &mut progress)?;
        flatten(&tmp, &source.unpack)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result.with_context(|| format!("解压 {}", source.archive.display()))
}

/// 上次中断留下的同目标临时目录，开工前清掉。
fn cleanup_stale(parent: &Path, name: &str) -> Result<()> {
    let prefix = format!(".{name}.tmp-");
    let entries = fs::read_dir(parent).with_context(|| format!("读不到 {}", parent.display()))?;
    for entry in entries {
        let entry = entry?;
        let Some(file) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if file.starts_with(&prefix) {
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}

/// 压缩包顶层如果只有一个目录，就把这一层提上来，
/// 这样 `unpack` 始终是源码根（configure / phpize 所在目录）。
fn flatten(tmp: &Path, dest: &Path) -> Result<()> {
    let entries: Vec<_> = fs::read_dir(tmp)
        .with_context(|| format!("读不到 {}", tmp.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    if entries.is_empty() {
        bail!("压缩包是空的");
    }
    if entries.len() == 1 {
        let only = &entries[0];
        if only.file_type()?.is_dir() {
            fs::rename(only.path(), dest)
                .with_context(|| format!("无法写入 {}", dest.display()))?;
            fs::remove_dir(tmp).with_context(|| format!("删不掉 {}", tmp.display()))?;
            return Ok(());
        }
    }
    // PECL 包是 `package.xml` + 一个源码目录；源码根是那个目录。
    let dirs: Vec<_> = entries
        .iter()
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    if dirs.len() == 1
        && entries.iter().all(|entry| {
            entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                || entry.file_name().to_string_lossy() == "package.xml"
        })
    {
        fs::rename(dirs[0].path(), dest).with_context(|| format!("无法写入 {}", dest.display()))?;
        fs::remove_dir_all(tmp).with_context(|| format!("删不掉 {}", tmp.display()))?;
        return Ok(());
    }
    fs::rename(tmp, dest).with_context(|| format!("无法写入 {}", dest.display()))?;
    Ok(())
}
