use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use phprt_source::Source;

use crate::layout;
use crate::recipe::Target;

/// 从求解结果生成清单：PHP 本体、依赖库、PECL 扩展。
/// builtin 扩展的源码在 PHP tarball 里，不单独下载或解压。
pub fn collect(order: &[&Target], root: &Path, cache: &Path) -> Result<Vec<Source>> {
    let mut sources = Vec::new();
    let mut seen: BTreeMap<PathBuf, String> = BTreeMap::new();

    for target in order {
        if !layout::downloadable(target) || target.url.is_empty() {
            continue;
        }
        let file_name = layout::archive_name(&target.url)
            .ok_or_else(|| anyhow::anyhow!("无法从 URL 得到缓存文件名: {}", target.url))?;
        let archive = cache.join(&file_name);
        if let Some(previous) = seen.insert(archive.clone(), target.url.clone())
            && previous != target.url
        {
            bail!(
                "缓存文件名冲突: {file_name}（{previous} 和 {}）",
                target.url
            );
        }
        let unpack_display = layout::build_dir(&target.name, target.kind, &target.url);
        sources.push(Source {
            label: layout::label(target.kind).to_owned(),
            name: target.name.clone(),
            version: target.version.clone(),
            url: target.url.clone(),
            sha256: target.sha256.clone(),
            archive,
            unpack: root.join(&unpack_display),
            unpack_display,
        });
    }
    Ok(sources)
}
