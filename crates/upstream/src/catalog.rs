//! catalog.toml 的读改写：插入新补丁、回填 sha256、备份与原子写入。
//!
//! 用 toml_edit 在内存文档上操作，保留注释、分组和数组里其它元素的格式；
//! 不能整体反序列化再序列化（docs/CATALOG-UPDATE.md 9）。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use toml_edit::{DocumentMut, Value};

/// 要写进 catalog 的一条补丁。
#[derive(Debug, Clone)]
pub struct Patch {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

/// 读取 catalog.toml 为可改写文档。
pub fn read(path: &Path) -> Result<DocumentMut> {
    let text = fs::read_to_string(path).with_context(|| format!("读不到 {}", path.display()))?;
    text.parse::<DocumentMut>()
        .with_context(|| format!("{} 不是合法的 catalog.toml", path.display()))
}

/// catalog 里声明的全部组件名（按 key 排序）。
pub fn components(doc: &DocumentMut) -> Vec<String> {
    doc.iter().map(|(name, _)| name.to_owned()).collect()
}

/// 组件的版本线名；组件不在 catalog 里时返回 None。
pub fn lines(doc: &DocumentMut, component: &str) -> Option<Vec<String>> {
    let table = doc.get(component)?.as_table()?;
    Some(table.iter().map(|(name, _)| name.to_owned()).collect())
}

/// 这条线当前的最大补丁；线不存在或数组为空时返回 None。
pub fn max_version(doc: &DocumentMut, component: &str, line: &str) -> Option<String> {
    let array = line_array(doc, component, line)?;
    array
        .iter()
        .filter_map(|value| {
            value
                .as_inline_table()
                .and_then(|table| table.get("version"))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .max_by(|left, right| crate::version::compare(left, right))
}

/// 缺 sha256 的已有条目：(组件, 线, 版本, url)。
pub fn missing_sha256(doc: &DocumentMut) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    for (component, item) in doc.iter() {
        let Some(table) = item.as_table() else {
            continue;
        };
        for (line, item) in table.iter() {
            let Some(array) = item.as_array() else {
                continue;
            };
            for value in array.iter() {
                let Some(entry) = value.as_inline_table() else {
                    continue;
                };
                let missing = entry
                    .get("sha256")
                    .and_then(|v| v.as_str())
                    .is_none_or(|sha| sha.is_empty());
                if !missing {
                    continue;
                }
                let version = entry
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                let url = entry
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned();
                out.push((component.to_owned(), line.to_owned(), version, url));
            }
        }
    }
    out
}

/// 在对应线的数组末尾插入一条补丁；数组顺序不参与选择（取最大版本），所以不重排。
/// 同一版本已存在时不写，返回 false。
pub fn append_patch(
    doc: &mut DocumentMut,
    component: &str,
    line: &str,
    patch: &Patch,
) -> Result<bool> {
    let array = line_array_mut(doc, component, line)?;
    for value in array.iter() {
        if value
            .as_inline_table()
            .and_then(|table| table.get("version"))
            .and_then(|v| v.as_str())
            == Some(patch.version.as_str())
        {
            return Ok(false);
        }
    }
    let mut table = toml_edit::InlineTable::new();
    table.insert("version", Value::from(patch.version.as_str()));
    table.insert("url", Value::from(patch.url.as_str()));
    table.insert("sha256", Value::from(patch.sha256.as_str()));
    let multiline = array.to_string().contains('\n');
    array.push(Value::InlineTable(table));
    let last = array.len() - 1;
    if multiline {
        // 前缀装饰要在 push 之后才生效：多行数组的新元素独占一行。
        if let Some(value) = array.get_mut(last) {
            value.decor_mut().set_prefix("\n    ");
        }
    } else if last > 0 {
        // 单行数组：原末元素尾部的空格会被逗号挤到前面，把它挪走，
        // 新元素前后各补一个空格，保持 `[ {..}, {..} ]` 的样子。
        if let Some(prev) = array.get_mut(last - 1) {
            prev.decor_mut().set_suffix("");
        }
        if let Some(value) = array.get_mut(last) {
            value.decor_mut().set_prefix(" ");
            value.decor_mut().set_suffix(" ");
        }
    }
    Ok(true)
}

/// 给已有条目补 sha256；已有非空值时不覆盖。返回是否写了。
pub fn fill_sha256(
    doc: &mut DocumentMut,
    component: &str,
    line: &str,
    version: &str,
    sha256: &str,
) -> Result<bool> {
    let array = line_array_mut(doc, component, line)?;
    for value in array.iter_mut() {
        let Some(table) = value.as_inline_table_mut() else {
            continue;
        };
        if table.get("version").and_then(|v| v.as_str()) != Some(version) {
            continue;
        }
        match table.get("sha256").and_then(|v| v.as_str()) {
            Some(existing) if !existing.is_empty() => return Ok(false),
            _ => {}
        }
        // 直接 insert 会把原表尾的空格留在逗号前面（渲染成 `" , sha256"`），
        // 按原顺序抄一遍再追加哈希，写进去才干净；inline table 自己的装饰
        //（`[ { ... } ]` 方括号内侧的空格）也要搬过去。
        let entries: Vec<(String, String)> = table
            .iter()
            .filter(|(key, _)| *key != "sha256")
            .filter_map(|(key, value)| value.as_str().map(|v| (key.to_owned(), v.to_owned())))
            .collect();
        let (prefix, suffix) = (
            table.decor().prefix().cloned(),
            table.decor().suffix().cloned(),
        );
        let mut fresh = toml_edit::InlineTable::new();
        for (key, value) in entries {
            fresh.insert(&key, Value::from(value));
        }
        fresh.insert("sha256", Value::from(sha256));
        *table = fresh;
        if let Some(prefix) = prefix {
            table.decor_mut().set_prefix(prefix);
        }
        if let Some(suffix) = suffix {
            table.decor_mut().set_suffix(suffix);
        }
        return Ok(true);
    }
    bail!("catalog 里没有 {component}.{line} 的补丁 {version}")
}

/// 写文件前备份一次：`<备份目录>/<UTC 时间戳>_catalog.toml`，和 ini 备份共用配置。
pub fn backup(path: &Path, dir: &Path) -> Result<PathBuf> {
    let bytes = fs::read(path).with_context(|| format!("读不了 {}", path.display()))?;
    fs::create_dir_all(dir).with_context(|| format!("建不了 {}", dir.display()))?;
    let name = format!("{}_catalog.toml", utc_stamp());
    let saved = unique_path(dir, &name);
    fs::write(&saved, &bytes).with_context(|| format!("写不了 {}", saved.display()))?;
    Ok(saved)
}

/// 把内存文档写回 catalog.toml；调用方负责先备份、用 Catalog::load 复检临时文件。
pub fn write(path: &Path, doc: &DocumentMut) -> Result<()> {
    let text = doc.to_string();
    fs::write(path, &text).with_context(|| format!("写不了 {}", path.display()))?;
    Ok(())
}

fn line_array_mut<'doc>(
    doc: &'doc mut DocumentMut,
    component: &str,
    line: &str,
) -> Result<&'doc mut toml_edit::Array> {
    doc.get_mut(component)
        .and_then(|item| item.as_table_mut())
        .and_then(|table| table.get_mut(line))
        .and_then(|item| item.as_array_mut())
        .with_context(|| format!("catalog 里没有 {component} 的 {line} 线"))
}

fn line_array<'doc>(
    doc: &'doc DocumentMut,
    component: &str,
    line: &str,
) -> Option<&'doc toml_edit::Array> {
    doc.get(component)
        .and_then(|item| item.as_table())
        .and_then(|table| table.get(line))
        .and_then(|item| item.as_array())
}

/// 同名文件依次加 `-2`、`-3`；序号放在扩展名前面（和 ini 备份一致）。
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(name);
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_owned(), format!(".{ext}")),
        None => (name.to_owned(), String::new()),
    };
    let mut index = 1;
    while path.exists() {
        index += 1;
        path = dir.join(format!("{stem}-{index}{ext}"));
    }
    path
}

/// UTC 时间戳，格式 YYYYMMDD-HHMMSS。
fn utc_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("系统时间早于 1970 年")
        .as_secs();
    let days = (secs / 86400) as i64;
    let secs_of_day = secs % 86400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        secs_of_day / 3600,
        secs_of_day % 3600 / 60,
        secs_of_day % 60
    )
}

/// 1970-01-01 起的儒略日转公历（Howard Hinnant 算法）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = r#"# 头部注释要保留

[openssl]
"3.5" = [
    { version = "3.5.8", url = "https://x/openssl-3.5.8.tar.gz" },
    { version = "3.5.9", url = "https://x/openssl-3.5.9.tar.gz", sha256 = "old" },
]
"1.1.1" = [ { version = "1.1.1w", url = "https://x/openssl-1.1.1w.tar.gz" } ]

[zlib]
"1.3" = [ { version = "1.3.1", url = "https://x/zlib-1.3.1.tar.gz" } ]
"#;

    fn doc() -> DocumentMut {
        CATALOG.parse::<DocumentMut>().unwrap()
    }

    #[test]
    fn append_keeps_comments_and_style() {
        let mut document = doc();
        let inserted = append_patch(
            &mut document,
            "openssl",
            "3.5",
            &Patch {
                version: "3.5.10".into(),
                url: "https://x/openssl-3.5.10.tar.gz".into(),
                sha256: "abc".into(),
            },
        )
        .unwrap();
        assert!(inserted);
        let text = document.to_string();
        assert!(text.starts_with("# 头部注释要保留"));
        assert!(text.contains(
            "{ version = \"3.5.9\", url = \"https://x/openssl-3.5.9.tar.gz\", sha256 = \"old\" },"
        ));
        assert!(text.contains("\n    { version = \"3.5.10\", url = \"https://x/openssl-3.5.10.tar.gz\", sha256 = \"abc\" },\n]"), "actual:\n{text}");
        // 单行数组追加后仍是单行风格。
        let mut document = doc();
        append_patch(
            &mut document,
            "zlib",
            "1.3",
            &Patch {
                version: "1.3.2".into(),
                url: "https://x/zlib-1.3.2.tar.gz".into(),
                sha256: "def".into(),
            },
        )
        .unwrap();
        let text = document.to_string();
        assert!(text.contains(r#""1.3" = [ { version = "1.3.1", url = "https://x/zlib-1.3.1.tar.gz" }, { version = "1.3.2", url = "https://x/zlib-1.3.2.tar.gz", sha256 = "def" } ]"#), "actual:\n{text}");
    }

    #[test]
    fn append_skips_an_existing_version() {
        let mut document = doc();
        let again = append_patch(
            &mut document,
            "openssl",
            "3.5",
            &Patch {
                version: "3.5.9".into(),
                url: "https://x/openssl-3.5.9.tar.gz".into(),
                sha256: "new".into(),
            },
        )
        .unwrap();
        assert!(!again);
        assert_eq!(document.to_string(), CATALOG);
    }

    #[test]
    fn append_reports_a_missing_line() {
        let mut document = doc();
        let err = append_patch(
            &mut document,
            "openssl",
            "4.0",
            &Patch {
                version: "4.0.1".into(),
                url: "u".into(),
                sha256: "s".into(),
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("没有 openssl 的 4.0 线"));
    }

    #[test]
    fn fill_only_touches_empty_hashes() {
        let mut document = doc();
        let written = fill_sha256(&mut document, "openssl", "3.5", "3.5.8", "new").unwrap();
        assert!(written);
        let untouched = fill_sha256(&mut document, "openssl", "3.5", "3.5.9", "newer").unwrap();
        assert!(!untouched);
        let text = document.to_string();
        assert!(
            text.contains(
                r#"version = "3.5.8", url = "https://x/openssl-3.5.8.tar.gz", sha256 = "new""#
            ),
            "actual:\n{text}"
        );
        assert!(text.contains(r#"sha256 = "old""#));
    }

    #[test]
    fn fill_keeps_single_line_array_style() {
        let mut document = doc();
        fill_sha256(&mut document, "openssl", "1.1.1", "1.1.1w", "abc").unwrap();
        let text = document.to_string();
        assert!(
            text.contains(
                r#""1.1.1" = [ { version = "1.1.1w", url = "https://x/openssl-1.1.1w.tar.gz", sha256 = "abc" } ]"#
            ),
            "actual:\n{text}"
        );
    }

    #[test]
    fn max_version_takes_the_latest_patch() {
        let document = doc();
        assert_eq!(max_version(&document, "openssl", "3.5").unwrap(), "3.5.9");
        assert_eq!(max_version(&document, "zlib", "1.3").unwrap(), "1.3.1");
        assert!(max_version(&document, "openssl", "4.0").is_none());
    }

    #[test]
    fn missing_sha256_lists_empty_and_absent_hashes() {
        let document = doc();
        let missing = missing_sha256(&document);
        assert_eq!(missing.len(), 3);
        assert!(missing.contains(&(
            "openssl".into(),
            "3.5".into(),
            "3.5.8".into(),
            "https://x/openssl-3.5.8.tar.gz".into()
        )));
        assert!(missing.contains(&(
            "openssl".into(),
            "1.1.1".into(),
            "1.1.1w".into(),
            "https://x/openssl-1.1.1w.tar.gz".into()
        )));
        assert!(missing.contains(&(
            "zlib".into(),
            "1.3".into(),
            "1.3.1".into(),
            "https://x/zlib-1.3.1.tar.gz".into()
        )));
    }

    #[test]
    fn backup_name_is_unique_and_stamped() {
        let dir = std::env::temp_dir().join(format!("phprt-upstream-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let first = backup(Path::new("Cargo.toml"), &dir).unwrap();
        let second = backup(Path::new("Cargo.toml"), &dir).unwrap();
        assert!(first.is_file() && second.is_file());
        assert_ne!(first, second);
        assert!(
            first
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("_catalog.toml")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn utc_stamp_format() {
        // 20000101-000000UTC 对应的儒略日偏移。
        let stamp = utc_stamp();
        assert_eq!(stamp.len(), 15);
        assert_eq!(&stamp[8..9], "-");
    }
}
