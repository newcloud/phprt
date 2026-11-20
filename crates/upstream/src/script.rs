//! upstream star 脚本的加载与执行。
//!
//! 契约（docs/CATALOG-UPDATE.md 4.1）：脚本顶层一个 UPSTREAM 字典，三个槽位——
//! - `url`：访问哪个地址能拿到版本和下载连接列表；`{line}` 会被替换成线
//!   （只有 URL 本身按线划分的源才写它，如 php.net）；
//! - `link`：从返回内容抽下载连接的正则，必须有命名组 `link`；
//! - `version`：从下载连接抽版本的正则，必须有命名组 `version`
//!   （`v2.1.33` 这类前缀在正则里过滤掉）；
//! - `join`（可选）：连接是相对地址时，拼成绝对地址的模板，如 `"https://x/{link}"`。
//!
//! 线归属、稳定性过滤、版本比较不在脚本里，在 Rust 侧（version.rs）。

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use regex::Regex;
use serde_json::Value;
use starlark::environment::{Globals, Module};
use starlark::eval::Evaluator;
use starlark::syntax::{AstModule, Dialect};

/// 一个组件的查询脚本解析结果。
#[derive(Debug, Clone)]
pub struct Script {
    pub component: String,
    /// 访问哪个地址；可能含 `{line}` 占位符。
    pub url: String,
    /// 从返回内容抽下载连接的正则（命名组 link）。
    pub link: Regex,
    /// 从下载连接抽版本的正则（命名组 version）。
    pub version: Regex,
    /// 相对连接拼成绝对地址的模板；没有则连接本身是绝对地址。
    pub join: Option<String>,
}

impl Script {
    /// 读取并执行 `recipes/upstream/<组件>.star`。
    pub fn load(recipes: &Path, component: &str) -> Result<Self> {
        let path = recipes.join("upstream").join(format!("{component}.star"));
        let src = std::fs::read_to_string(&path)
            .with_context(|| format!("读不到 upstream 脚本 {}", path.display()))?;
        Self::parse(component, &src)
    }

    /// 执行脚本源码，取出 UPSTREAM 字典。
    pub fn parse(component: &str, src: &str) -> Result<Self> {
        let ast = AstModule::parse("upstream.star", src.to_owned(), &Dialect::Standard)
            .map_err(|err| anyhow!("upstream 脚本解析失败: {err}"))?;
        let json = Module::with_temp_heap(|module| {
            let mut eval = Evaluator::new(&module);
            eval.eval_module(ast, &Globals::standard())
                .map_err(|err| anyhow!("upstream 脚本求值失败: {err}"))?;
            let value = module
                .get("UPSTREAM")
                .ok_or_else(|| anyhow!("upstream 脚本没有定义顶层 UPSTREAM"))?;
            value
                .to_json_value()
                .map_err(|err| anyhow!("UPSTREAM 转 JSON 失败: {err}"))
        })?;
        let fields = json
            .as_object()
            .ok_or_else(|| anyhow!("UPSTREAM 必须是字典"))?;
        let url = string_field(fields, "url")?.to_owned();
        let link = Regex::new(string_field(fields, "link")?)
            .with_context(|| format!("{component} 的 link 不是合法正则"))?;
        let version = Regex::new(string_field(fields, "version")?)
            .with_context(|| format!("{component} 的 version 不是合法正则"))?;
        require_group(&link, "link", component)?;
        require_group(&version, "version", component)?;
        let join = match fields.get("join") {
            Some(Value::String(join)) => Some(join.clone()),
            Some(_) => bail!("{component} 的 join 必须是字符串"),
            None => None,
        };
        Ok(Self {
            component: component.to_owned(),
            url,
            link,
            version,
            join,
        })
    }

    /// 把 `{line}` 替换成线；脚本 URL 不带占位符时忽略线。
    pub fn url_for(&self, line: Option<&str>) -> Result<String> {
        if self.url.contains("{line}") {
            let line = line
                .ok_or_else(|| anyhow!("{} 的 URL 带 {{line}}，但查询时没给线", self.component))?;
            Ok(self.url.replace("{line}", line))
        } else {
            Ok(self.url.clone())
        }
    }
}

fn string_field<'a>(fields: &'a serde_json::Map<String, Value>, name: &str) -> Result<&'a str> {
    fields
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("UPSTREAM 缺少字符串字段 {name}"))
}

fn require_group(regex: &Regex, group: &str, component: &str) -> Result<()> {
    regex
        .capture_names()
        .any(|name| name == Some(group))
        .then_some(())
        .ok_or_else(|| anyhow!("{component} 的正则必须有命名组 {group}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZLIB: &str = r#"
UPSTREAM = {
    "url": "https://zlib.net/fossils/",
    "link": r'href="(?P<link>zlib-[0-9.]+\.tar\.gz)"',
    "version": r'zlib-(?P<version>[0-9.]+)\.tar\.gz',
    "join": "https://zlib.net/fossils/{link}",
}
"#;

    #[test]
    fn parses_the_three_slots() {
        let script = Script::parse("zlib", ZLIB).unwrap();
        assert_eq!(script.url, "https://zlib.net/fossils/");
        assert_eq!(
            script.join.as_deref(),
            Some("https://zlib.net/fossils/{link}")
        );
        assert!(script.url_for(None).unwrap().contains("fossils"));
    }

    #[test]
    fn link_and_version_regexes_agree_on_a_listing_row() {
        let script = Script::parse("zlib", ZLIB).unwrap();
        let body = r#"<a href="zlib-1.3.2.tar.gz">zlib-1.3.2.tar.gz</a>"#;
        let link = script
            .link
            .captures(body)
            .and_then(|cap| cap.name("link"))
            .map(|m| m.as_str().to_owned())
            .unwrap();
        let version = script
            .version
            .captures(&link)
            .and_then(|cap| cap.name("version"))
            .map(|m| m.as_str().to_owned())
            .unwrap();
        assert_eq!(link, "zlib-1.3.2.tar.gz");
        assert_eq!(version, "1.3.2");
    }

    #[test]
    fn version_regex_can_strip_the_v_prefix() {
        let src = r#"
UPSTREAM = {
    "url": "https://example.com/tags",
    "link": r'(?P<link>v[0-9.]+)',
    "version": r'v(?P<version>[0-9.]+)',
}
"#;
        let script = Script::parse("demo", src).unwrap();
        let version = script
            .version
            .captures("v2.1.33")
            .and_then(|cap| cap.name("version"))
            .map(|m| m.as_str().to_owned())
            .unwrap();
        assert_eq!(version, "2.1.33");
    }

    #[test]
    fn missing_named_groups_are_rejected() {
        let src = r#"
UPSTREAM = {
    "url": "https://example.com/",
    "link": r'(?P<href>[^"]+)',
    "version": r'(?P<version>[0-9.]+)',
}
"#;
        assert!(Script::parse("demo", src).is_err());
    }

    #[test]
    fn url_with_line_placeholder_requires_a_line() {
        let src = r#"
UPSTREAM = {
    "url": "https://example.com/releases/index.php?json&version={line}",
    "link": r'(?P<link>[^"]+)',
    "version": r'(?P<version>[0-9.]+)',
}
"#;
        let script = Script::parse("demo", src).unwrap();
        assert!(script.url_for(None).is_err());
        assert_eq!(
            script.url_for(Some("8.5")).unwrap(),
            "https://example.com/releases/index.php?json&version=8.5"
        );
    }
}
