use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use phprt_source::Source;

use crate::layout;
use crate::recipe::Target;

/// 从求解结果生成清单：产品本体、依赖库、外部组件。
/// bundled 组件的源码在产品 tarball 里，不单独下载或解压。
pub fn collect(order: &[&Target], root: &Path, cache: &Path) -> Result<Vec<Source>> {
    let mut sources = Vec::new();
    let mut seen: BTreeMap<PathBuf, String> = BTreeMap::new();

    for target in order {
        if !layout::downloadable(target) || target.url.is_empty() {
            continue;
        }
        let archive = layout::archive_path(cache, &target.name, &target.url)
            .ok_or_else(|| anyhow::anyhow!("无法从 URL 得到缓存文件名: {}", target.url))?;
        let file_name = archive
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("?")
            .to_owned();
        if let Some(previous) = seen.insert(archive.clone(), target.url.clone())
            && previous != target.url
        {
            bail!(
                "缓存文件名冲突: {file_name}（{previous} 和 {}）",
                target.url
            );
        }
        let unpack_display = layout::build_dir(&target.name, &target.url);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::Kind;

    fn target(kind: Kind, name: &str, url: &str) -> Target {
        Target {
            kind,
            name: name.into(),
            url: url.into(),
            ..Target::default()
        }
    }

    #[test]
    fn archives_are_grouped_by_item_name() {
        let root = Path::new("/root");
        let cache = Path::new("/cache");
        let avif = target(
            Kind::Dep,
            "libavif",
            "https://example.com/avif/archive/v1.3.0.tar.gz",
        );
        let uhdr = target(
            Kind::Dep,
            "libultrahdr",
            "https://example.com/uhdr/archive/v1.3.0.tar.gz",
        );

        let sources = collect(&[&avif, &uhdr], root, cache).unwrap();
        assert_eq!(sources[0].archive, cache.join("libavif/v1.3.0.tar.gz"));
        assert_eq!(sources[1].archive, cache.join("libultrahdr/v1.3.0.tar.gz"));
    }

    #[test]
    fn same_item_same_file_name_still_conflicts() {
        let root = Path::new("/root");
        let cache = Path::new("/cache");
        let a = target(Kind::Dep, "foo", "https://example.com/a/v1.0.tar.gz");
        let b = target(Kind::Dep, "foo", "https://example.com/b/v1.0.tar.gz");

        let err = collect(&[&a, &b], root, cache).unwrap_err().to_string();
        assert!(err.contains("缓存文件名冲突"), "{err}");
    }
}
