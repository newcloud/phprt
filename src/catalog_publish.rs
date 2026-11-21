use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::catalog::{Artifact, Catalog};

/// `catalog publish` 的输出摘要。
#[derive(Debug)]
pub struct Summary {
    pub components: usize,
    pub out: PathBuf,
    pub stamp: String,
}

/// 把单文件 catalog 拆成每个组件一个分片。
///
/// 只做数据变换，不访问上游；每条补丁都必须有 url 和 sha256，缺一条就整体失败。
pub fn run(catalog_path: &Path, out: &Path) -> Result<Summary> {
    let catalog = Catalog::load(catalog_path)?;
    let missing = missing_sha256(&catalog);
    if !missing.is_empty() {
        let shown = missing
            .iter()
            .take(10)
            .map(|item| item.as_str())
            .collect::<Vec<_>>()
            .join("\n  ");
        bail!(
            "catalog 里有 {} 条补丁缺少 sha256，不能发布：\n  {shown}\n\
             先跑 phprt catalog update --fill-sha256",
            missing.len()
        );
    }

    let components_dir = out.join("latest").join("components");
    fs::create_dir_all(&components_dir)
        .with_context(|| format!("建不了 {}", components_dir.display()))?;

    let mut written = std::collections::BTreeSet::new();
    for (name, lines) in &catalog.components {
        check_component_name(name)?;
        let text = shard_text(name, lines);
        let path = components_dir.join(format!("{name}.toml"));
        fs::write(&path, text).with_context(|| format!("写不了 {}", path.display()))?;
        written.insert(format!("{name}.toml"));
    }

    // 上一次发布里多出来的组件不能继续留在目录里，否则会和新数据混在一起。
    for entry in fs::read_dir(&components_dir)
        .with_context(|| format!("读不到 {}", components_dir.display()))?
    {
        let path = entry?.path();
        let Some(file) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if file.ends_with(".toml") && !written.contains(file) {
            fs::remove_file(&path).with_context(|| format!("删不了 {}", path.display()))?;
        }
    }

    let stamp = utc_now();
    let stamp_path = out.join("latest").join("stamp");
    fs::write(&stamp_path, format!("{stamp}\n"))
        .with_context(|| format!("写不了 {}", stamp_path.display()))?;

    Ok(Summary {
        components: written.len(),
        out: out.to_path_buf(),
        stamp,
    })
}

fn missing_sha256(catalog: &Catalog) -> Vec<String> {
    let mut out = Vec::new();
    for (name, lines) in &catalog.components {
        for (line, artifacts) in lines {
            for artifact in artifacts {
                if artifact.sha256.trim().is_empty() {
                    out.push(format!("{name}.{line}.{}", artifact.version));
                }
            }
        }
    }
    out
}

pub(crate) fn check_component_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '+'))
    {
        bail!("组件名 {name:?} 不能作为分片文件名和 TOML 表名");
    }
    Ok(())
}

fn shard_text(name: &str, lines: &BTreeMap<String, Vec<Artifact>>) -> String {
    let mut out = format!("[{}]\n", table_name(name));
    for (line, artifacts) in lines {
        let mut sorted = artifacts.clone();
        sorted.sort_by(|left, right| phprt_upstream::compare(&left.version, &right.version));
        out.push_str(&format!("{line:?} = [\n"));
        for artifact in sorted {
            out.push_str(&format!(
                "    {{ version = {:?}, url = {:?}, sha256 = {:?} }},\n",
                artifact.version, artifact.url, artifact.sha256
            ));
        }
        out.push_str("]\n");
    }
    out
}

/// 简单名字用裸 key；带点或加号时用引号，避免被当成嵌套表。
fn table_name(name: &str) -> String {
    if name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        name.to_owned()
    } else {
        format!("{name:?}")
    }
}

/// UTC 时间，`2026-11-21T03:20:00Z`；不引入时间库。
pub(crate) fn utc_now() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    utc_from_unix(seconds)
}

fn utc_from_unix(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let rem = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Howard Hinnant 的 civil_from_days。
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(path: &Path, text: &str) {
        fs::write(path, text).unwrap();
    }

    #[test]
    fn publish_writes_one_shard_per_component() {
        let root = std::env::temp_dir().join(format!(
            "phprt-publish-test-{}-{}",
            std::process::id(),
            utc_now()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = root.join("catalog.toml");
        catalog(
            &source,
            r#"
[openssl]
"3.5" = [
    { version = "3.5.9", url = "https://example.com/3.5.9.tar.gz", sha256 = "bb" },
    { version = "3.5.10", url = "https://example.com/3.5.10.tar.gz", sha256 = "aa" },
]

[zlib]
"1.3" = [ { version = "1.3.2", url = "https://example.com/zlib.tar.gz", sha256 = "cc" } ]
"#,
        );

        let out = root.join("dist");
        let summary = run(&source, &out).unwrap();
        assert_eq!(summary.components, 2);

        let openssl = fs::read_to_string(out.join("latest/components/openssl.toml")).unwrap();
        assert!(openssl.starts_with("[openssl]\n"));
        // 分片必须能按 catalog 重新解析。
        let parsed = Catalog::parse(&openssl, "published").unwrap();
        assert_eq!(parsed.get("openssl", "3.5").unwrap().version, "3.5.10");
        // 补丁按版本升序，输出稳定。
        let first = openssl.find("3.5.9").unwrap();
        let second = openssl.find("3.5.10").unwrap();
        assert!(first < second);
        assert!(out.join("latest/components/zlib.toml").is_file());
        assert!(out.join("latest/stamp").is_file());

        // 再发一次，分片字节不变；上次多出来的组件会被清掉。
        let stale = out.join("latest/components/old.toml");
        fs::write(&stale, "[old]\n").unwrap();
        let before = fs::read(out.join("latest/components/openssl.toml")).unwrap();
        run(&source, &out).unwrap();
        let after = fs::read(out.join("latest/components/openssl.toml")).unwrap();
        assert_eq!(before, after);
        assert!(!stale.exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn publish_refuses_missing_sha256() {
        let root = std::env::temp_dir().join(format!(
            "phprt-publish-missing-{}-{}",
            std::process::id(),
            utc_now()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = root.join("catalog.toml");
        catalog(
            &source,
            r#"
[openssl]
"3.5" = [ { version = "3.5.9", url = "https://example.com/3.5.9.tar.gz" } ]
"#,
        );
        let err = run(&source, &root.join("dist")).unwrap_err().to_string();
        assert!(err.contains("openssl.3.5.3.5.9"), "{err}");
        assert!(!root.join("dist/latest/components/openssl.toml").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stamp_is_utc_rfc3339() {
        assert_eq!(utc_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_from_unix(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn component_name_must_be_a_safe_file_name() {
        assert!(check_component_name("openssl").is_ok());
        assert!(check_component_name("lua-resty-core").is_ok());
        assert!(check_component_name("foo.bar").is_ok());
        for bad in ["", ".", "..", "../evil", "a/b", "a\\b", "a b"] {
            assert!(check_component_name(bad).is_err(), "{bad:?}");
        }
    }
}
