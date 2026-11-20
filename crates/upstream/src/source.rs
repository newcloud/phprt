//! 按脚本查上游，输出候选 (版本, 下载连接)。

use std::collections::BTreeSet;

use anyhow::Result;

use crate::script::Script;

/// 脚本从上游抽出的一个候选。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub version: String,
    pub url: String,
}

/// 执行脚本的一次查询：访问 url，抽 (版本, 下载连接) 候选，按版本去重。
///
/// 同一组件的多条线共享同一次查询的结果（docs/CATALOG-UPDATE.md 5 的执行原则）；
/// URL 带 `{line}` 的源按线查询，那是源形态决定的。
pub fn query(
    script: &Script,
    line: Option<&str>,
    opts: &phprt_download::Options,
) -> Result<Vec<Candidate>> {
    let url = script.url_for(line)?;
    let (_final_url, body) = phprt_download::fetch_text(&url, opts)?;
    Ok(extract(script, &body))
}

/// 从返回内容里抽候选。纯函数，单测覆盖这条路径；联网只在 `query` 里。
pub fn extract(script: &Script, body: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for cap in script.link.captures_iter(body) {
        let Some(link) = cap.name("link").map(|m| m.as_str().to_owned()) else {
            continue;
        };
        let Some(version) = script
            .version
            .captures(&link)
            .and_then(|cap| cap.name("version"))
            .map(|m| m.as_str().to_owned())
        else {
            continue;
        };
        if version.is_empty() || !seen.insert(version.clone()) {
            continue;
        }
        let url = match &script.join {
            Some(template) => template.replace("{link}", &link),
            None => link,
        };
        out.push(Candidate { version, url });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version;

    fn script(src: &str) -> Script {
        Script::parse("demo", src).unwrap()
    }

    #[test]
    fn extracts_absolute_links_without_join() {
        let script = script(
            r#"
UPSTREAM = {
    "url": "https://api.github.com/repos/o/r/releases?per_page=100",
    "link": r'"browser_download_url":\s*"(?P<link>[^"]+\.tar\.gz)"',
    "version": r'o-(?P<version>[0-9.]+)\.tar\.gz',
}
"#,
        );
        let body = r#"{"assets": [{"browser_download_url": "https://x/o-1.2.3.tar.gz"}]}"#;
        let list = extract(&script, body);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].version, "1.2.3");
        assert_eq!(list[0].url, "https://x/o-1.2.3.tar.gz");
    }

    #[test]
    fn joins_relative_links_and_dedupes_versions() {
        let script = script(
            r#"
UPSTREAM = {
    "url": "https://example.com/fossils/",
    "link": r'href="(?P<link>z-[0-9.]+\.tar\.gz)"',
    "version": r'z-(?P<version>[0-9.]+)\.tar\.gz',
    "join": "https://example.com/fossils/{link}",
}
"#,
        );
        let body = r#"<a href="z-1.0.tar.gz">z-1.0.tar.gz</a><a href="z-1.0.tar.gz">again</a>"#;
        let list = extract(&script, body);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].url, "https://example.com/fossils/z-1.0.tar.gz");
    }

    #[test]
    fn links_without_a_version_are_dropped() {
        let script = script(
            r#"
UPSTREAM = {
    "url": "https://example.com/",
    "link": r'href="(?P<link>[^"]+)"',
    "version": r'z-(?P<version>[0-9.]+)\.tar\.gz',
}
"#,
        );
        let body = r#"<a href="README.md">README</a>"#;
        assert!(extract(&script, body).is_empty());
    }

    #[test]
    fn strips_the_v_prefix_in_the_version_regex() {
        let script = script(
            r#"
UPSTREAM = {
    "url": "https://example.com/tags",
    "link": r'"(?P<link>v[0-9.]+)"',
    "version": r'v(?P<version>[0-9.]+)',
}
"#,
        );
        let list = extract(&script, r#""v2.1.33""#);
        assert_eq!(list[0].version, "2.1.33");
    }

    #[test]
    fn pecl_rc_suffix_survives_extraction_for_the_unstable_filter() {
        // PECL 会把 RC 标成 stable（igbinary 3.2.17RC1 实测），版本正则必须保留
        // 后缀，交给 is_unstable 过滤；截掉后缀会让 RC 混进 catalog。
        let script = script(
            r#"
UPSTREAM = {
    "url": "https://pecl.php.net/rest/r/demo/allreleases.xml",
    "link": r'<r><v>(?P<link>[^<]+)</v><s>stable</s></r>',
    "version": r'(?P<version>[0-9]+(?:\.[0-9]+)*[a-zA-Z]*[0-9]*)',
}
"#,
        );
        let body = r#"<r><v>3.2.17RC1</v><s>stable</s></r><r><v>3.2.16</v><s>stable</s></r>"#;
        let list = extract(&script, body);
        assert_eq!(
            list.iter().map(|c| c.version.as_str()).collect::<Vec<_>>(),
            ["3.2.17RC1", "3.2.16"]
        );
        assert!(version::is_unstable("3.2.17RC1"));
        assert!(!version::is_unstable("3.2.16"));
    }
}
