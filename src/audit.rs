use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use serde::Serialize;

use crate::bundle::Bundle;
use crate::layout;
use crate::recipe::{Kind, Registry, Target, feature_available};

/// 一个上游选项在当前 recipe 里的处置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposal {
    /// 写在 `configure` 或 `dep_configure` 里。
    Configure,
    /// 做成扩展自己的 feature。
    Feature(String),
    /// 明确不暴露，并给出理由。
    Ignored(String),
    /// recipe 里没有任何处置。
    Missing,
}

#[derive(Debug, Clone)]
pub struct OptionRow {
    pub option: String,
    pub disposal: Disposal,
}

impl Serialize for OptionRow {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("option", &self.option)?;
        match &self.disposal {
            Disposal::Configure => map.serialize_entry("state", "configure")?,
            Disposal::Feature(name) => {
                map.serialize_entry("state", "feature")?;
                map.serialize_entry("detail", name)?;
            }
            Disposal::Ignored(reason) => {
                map.serialize_entry("state", "ignored")?;
                map.serialize_entry("detail", reason)?;
            }
            Disposal::Missing => map.serialize_entry("state", "missing")?,
        }
        map.end()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureRow {
    pub name: String,
    pub flags: Vec<String>,
    /// 形如 `php 8.5/8.6，dep:openssl 3.5/4.0`；无条件下为空。
    pub requires: String,
    /// 当前系列 bundle 里是否默认开启。
    pub default_on: bool,
    /// 当前 PHP / 依赖线是否满足 requires。
    pub available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub id: String,
    /// 源码目录；相对项目根。
    pub source: String,
    /// 上游选项清单；拿不到时为空，并在 note 里说明。
    pub options: Vec<OptionRow>,
    /// ignored 里写了、但上游没有这个选项（多半是拼错）。
    pub stale: Vec<(String, String)>,
    pub features: Vec<FeatureRow>,
    pub note: String,
    pub is_ext: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub series: String,
    pub components: Vec<Component>,
}

pub struct Options<'a> {
    pub root: &'a Path,
    pub series: &'a str,
    pub bundle: &'a Bundle,
}

impl Report {
    /// 扩展 recipe 里没有处置的上游选项；依赖库只提示，不算失败。
    pub fn missing(&self) -> Vec<(&Component, &OptionRow)> {
        self.components
            .iter()
            .filter(|component| component.is_ext)
            .flat_map(|component| {
                component
                    .options
                    .iter()
                    .filter(|row| row.disposal == Disposal::Missing)
                    .map(move |row| (component, row))
            })
            .collect()
    }
}

pub fn run(reg: &Registry, opts: &Options<'_>) -> Result<Report> {
    let mut components = Vec::new();
    for target in reg.targets.values() {
        if target.kind == Kind::Php {
            continue;
        }
        let (source, note, upstream) = upstream_options(reg, target, opts.root);
        let upstream_names: BTreeSet<String> = upstream
            .iter()
            .filter_map(|option| option_name(option))
            .collect();

        let mut options = Vec::new();
        for option in &upstream {
            let Some(name) = option_name(option) else {
                continue;
            };
            let disposal = dispose(target, &name);
            options.push(OptionRow {
                option: option.clone(),
                disposal,
            });
        }
        options.sort_by(|a, b| a.option.cmp(&b.option));

        let mut stale = Vec::new();
        if !upstream_names.is_empty() {
            for (flag, reason) in &target.ignored {
                let known = option_name(flag)
                    .map(|name| upstream_names.contains(&name))
                    .unwrap_or(false);
                if !known {
                    stale.push((flag.clone(), reason.clone()));
                }
            }
        }
        stale.sort();

        let mut features = Vec::new();
        for (name, def) in &target.features {
            features.push(FeatureRow {
                name: name.clone(),
                flags: def.configure.clone(),
                requires: describe_requires(def),
                default_on: opts
                    .bundle
                    .default_exts
                    .get(&target.name)
                    .map(|d| d.features.contains(name))
                    .unwrap_or(false),
                available: feature_available(reg, target, name).is_ok(),
            });
        }

        components.push(Component {
            id: target.id(),
            source,
            options,
            stale,
            features,
            note,
            is_ext: target.kind == Kind::Ext,
        });
    }
    components.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(Report {
        series: opts.series.to_owned(),
        components,
    })
}

/// 找上游源码里的 configure 选项。
///
/// PHP 扩展（内置 / PECL）读 `config.m4` 或 `config0.m4`；
/// 依赖库能跑 `./configure --help` 就用它，否则退回解析 `configure.ac`。
fn upstream_options(reg: &Registry, target: &Target, root: &Path) -> (String, String, Vec<String>) {
    match target.kind {
        Kind::Ext => {
            let dir = if target.origin == "builtin" {
                let php = reg.php().expect("registry 一定有 php");
                let relative = layout::build_dir(&php.name, Kind::Php, &php.url);
                PathBuf::from(root)
                    .join(relative)
                    .join("ext")
                    .join(&target.name)
            } else {
                PathBuf::from(root).join(layout::build_dir(&target.name, Kind::Ext, &target.url))
            };
            let relative = relative_path(root, &dir);
            if !dir.exists() {
                return (
                    relative,
                    "源码还没解压，先安装一次再跑 audit".to_owned(),
                    Vec::new(),
                );
            }
            for name in ["config.m4", "config0.m4"] {
                if let Ok(text) = fs::read_to_string(dir.join(name)) {
                    return (relative, String::new(), m4_options(&text));
                }
            }
            (
                relative,
                "源码里没有 config.m4 / config0.m4，先安装一次再跑 audit".to_owned(),
                Vec::new(),
            )
        }
        Kind::Dep => {
            let relative = layout::build_dir(&target.name, Kind::Dep, &target.url);
            let dir = PathBuf::from(root).join(&relative);
            if !dir.exists() {
                return (
                    relative,
                    "源码还没解压，先安装一次再跑 audit".to_owned(),
                    Vec::new(),
                );
            }
            if dir.join("configure").is_file()
                && let Some(options) = help_options(&dir)
            {
                return (relative, String::new(), options);
            }
            if let Ok(text) = fs::read_to_string(dir.join("configure.ac"))
                && !text.is_empty()
            {
                return (relative, String::new(), m4_options(&text));
            }
            if dir.join("Configure").is_file() {
                return (
                    relative,
                    "上游用 ./Configure，没有可解析的 --help 清单；ssl 等选项由 recipe 固定"
                        .to_owned(),
                    Vec::new(),
                );
            }
            (
                relative,
                "拿不到上游选项清单（没有 ./configure --help，也没有 configure.ac）".to_owned(),
                Vec::new(),
            )
        }
        Kind::Php => unreachable!(),
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

fn help_options(dir: &Path) -> Option<Vec<String>> {
    let output = Command::new("./configure")
        .arg("--help")
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(help_options_from_text(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub fn help_options_from_text(text: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim_start();
        let Some(token) = line.split_whitespace().next() else {
            continue;
        };
        let Some(rest) = token.strip_prefix("--") else {
            continue;
        };
        let rest = rest.split(['[', '=']).next().unwrap_or(rest);
        if rest.is_empty()
            || !rest
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            continue;
        }
        names.insert(format!("--{rest}"));
    }
    names.into_iter().collect()
}

/// 解析 `PHP_ARG_ENABLE(name, ...)`、`PHP_ARG_WITH([name], ...)`、`AC_ARG_*`。
pub fn m4_options(text: &str) -> Vec<String> {
    let mut names = BTreeSet::new();
    for (index, _) in text.match_indices("ARG_") {
        let rest = &text[index..];
        let (kind, rest) = if let Some(rest) = rest.strip_prefix("ARG_ENABLE(") {
            ("enable", rest)
        } else if let Some(rest) = rest.strip_prefix("ARG_WITH(") {
            ("with", rest)
        } else {
            continue;
        };
        let rest = rest.strip_prefix('[').unwrap_or(rest);
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if name.is_empty() || name.chars().any(|c| c.is_ascii_uppercase()) {
            continue;
        }
        names.insert(format!("--{kind}-{}", name.replace('_', "-")));
    }
    names.into_iter().collect()
}

fn option_name(option: &str) -> Option<String> {
    let option = option.trim();
    let option = option.split(['=', '[']).next().unwrap_or(option);
    let rest = option.strip_prefix("--")?;
    for prefix in ["enable-", "disable-", "with-", "without-"] {
        if let Some(name) = rest.strip_prefix(prefix) {
            return Some(name.to_owned());
        }
    }
    None
}

/// 一个上游选项（已经归一化成名字）在 recipe 里的处置。
fn dispose(target: &Target, name: &str) -> Disposal {
    if let Some(feature) = feature_using(target, name) {
        return Disposal::Feature(feature);
    }
    if let Some(reason) = ignored_reason(target, name) {
        return Disposal::Ignored(reason);
    }
    if declared(target).contains(name) {
        return Disposal::Configure;
    }
    Disposal::Missing
}

/// recipe 里写死的 configure 参数。
fn declared(target: &Target) -> BTreeSet<String> {
    target
        .configure
        .iter()
        .filter_map(|flag| option_name(flag))
        .collect()
}

fn feature_using(target: &Target, name: &str) -> Option<String> {
    for (feature, def) in &target.features {
        for flag in def.configure.iter().chain(def.disable.iter()) {
            if option_name(flag).as_deref() == Some(name) {
                return Some(feature.clone());
            }
        }
    }
    for (dep, flags) in &target.dep_configure {
        for flag in flags {
            if option_name(flag).as_deref() == Some(name) {
                return Some(dep.clone());
            }
        }
    }
    None
}

fn ignored_reason(target: &Target, name: &str) -> Option<String> {
    target
        .ignored
        .iter()
        .find(|(flag, _)| option_name(flag).as_deref() == Some(name))
        .map(|(_, reason)| reason.clone())
}

fn describe_requires(def: &crate::recipe::Feature) -> String {
    let mut parts = Vec::new();
    if !def.requires.php.is_empty() {
        parts.push(format!("php {}", def.requires.php.join("/")));
    }
    for (dep, lines) in &def.requires.dep {
        parts.push(format!("dep:{dep} {}", lines.join("/")));
    }
    parts.join("，")
}

pub fn text(report: &Report) -> String {
    let mut out = format!("audit PHP {}\n", report.series);
    for component in &report.components {
        out.push_str(&format!("\n{}\n", component.id));
        if !component.source.is_empty() {
            out.push_str(&format!("  source  {}\n", component.source));
        }
        if !component.note.is_empty() {
            out.push_str(&format!("  note    {}\n", component.note));
        }

        if component.is_ext {
            for row in &component.options {
                let text = match &row.disposal {
                    Disposal::Configure => "configure".to_owned(),
                    Disposal::Feature(name) => format!("feature {name}"),
                    Disposal::Ignored(reason) => format!("ignored: {reason}"),
                    Disposal::Missing => "未处理".to_owned(),
                };
                out.push_str(&format!("  {:<34} {}\n", row.option, text));
            }
        } else if !component.options.is_empty() {
            let fixed = component
                .options
                .iter()
                .filter(|row| row.disposal == Disposal::Missing)
                .count();
            out.push_str(&format!(
                "  上游 {} 个选项，其中 {fixed} 个没做成 feature（依赖库的其余选项由 recipe 固定）\n",
                component.options.len()
            ));
        }

        for (flag, reason) in &component.stale {
            out.push_str(&format!(
                "  {flag:<34} ignored 但上游没有这个选项：{reason}\n"
            ));
        }
        for feature in &component.features {
            let status = if !feature.available {
                "不可用"
            } else if feature.default_on {
                "默认开"
            } else {
                "未默认"
            };
            let flags = if feature.flags.is_empty() {
                String::new()
            } else {
                format!("  {}", feature.flags.join(" "))
            };
            let requires = if feature.requires.is_empty() {
                String::new()
            } else {
                format!("  requires {}", feature.requires)
            };
            out.push_str(&format!(
                "  feature {:<24} {status}{requires}{flags}\n",
                feature.name
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m4_options_reads_bracketed_and_bare_php_args() {
        let text = "PHP_ARG_WITH([openssl-legacy-provider], ...)\n\
                    PHP_ARG_ENABLE(xdebug, ...)\n\
                    PHP_ARG_WITH([system-ciphers], ...)";
        assert_eq!(
            m4_options(text),
            vec![
                "--enable-xdebug",
                "--with-openssl-legacy-provider",
                "--with-system-ciphers"
            ]
        );
    }

    #[test]
    fn m4_options_skips_uppercase_placeholders() {
        assert!(m4_options("AC_ARG_ENABLE(FEATURE, ...)").is_empty());
        assert!(m4_options("AC_ARG_WITH([ARG], ...)").is_empty());
    }

    #[test]
    fn help_options_reads_autotools_output() {
        let text = "  --enable-debug          turn on debugging\n\
                      --with-ssl[=DIR]        use OpenSSL\n\
                      --disable-something\n\
                      --enable-FEATURE[=ARG]\n\
                    Some text --not-an-option\n";
        assert_eq!(
            help_options_from_text(text),
            vec!["--disable-something", "--enable-debug", "--with-ssl"]
        );
    }

    fn target() -> Target {
        let mut target = Target {
            kind: Kind::Ext,
            name: "openssl".into(),
            ..blank()
        };
        target.configure = vec!["--with-openssl={dep:openssl}".into()];
        target.features.insert(
            "argon2".into(),
            crate::recipe::Feature {
                configure: vec!["--with-openssl-argon2".into()],
                ..Default::default()
            },
        );
        target.ignored.insert(
            "--with-openssl-legacy-provider".into(),
            "会改变默认 TLS 行为".into(),
        );
        target
    }

    fn blank() -> Target {
        Target {
            kind: Kind::Dep,
            name: String::new(),
            version: String::new(),
            url: String::new(),
            sha256: String::new(),
            needs: vec![],
            tools: vec![],
            builtin_exts: vec![],
            builtin_disable: Default::default(),
            exts: vec![],
            modules: vec![],
            origin: "builtin".into(),
            link: vec![],
            dep_configure: Default::default(),
            shared_from: String::new(),
            ini: String::new(),
            ini_lines: vec![],
            features: Default::default(),
            ignored: Default::default(),
            configure: vec![],
            make: vec![],
            artifacts: vec![],
            desc: String::new(),
            line: String::new(),
            series: String::new(),
        }
    }

    #[test]
    fn dispose_covers_configure_feature_ignored_and_missing() {
        let target = target();
        assert_eq!(dispose(&target, "openssl"), Disposal::Configure);
        assert_eq!(
            dispose(&target, "openssl-argon2"),
            Disposal::Feature("argon2".into())
        );
        assert_eq!(
            dispose(&target, "openssl-legacy-provider"),
            Disposal::Ignored("会改变默认 TLS 行为".into())
        );
        assert_eq!(dispose(&target, "system-ciphers"), Disposal::Missing);
    }

    #[test]
    fn option_names_drop_prefix_and_value() {
        assert_eq!(option_name("--with-openssl=/x").as_deref(), Some("openssl"));
        assert_eq!(option_name("--disable-brotli").as_deref(), Some("brotli"));
        assert_eq!(
            option_name("--enable-swoole-curl").as_deref(),
            Some("swoole-curl")
        );
        assert_eq!(option_name("--prefix=/x"), None);
        assert_eq!(option_name("./configure"), None);
    }
}
