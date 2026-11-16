use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::recipe::{Target, ToolSpec};

/// 检查产品工具和本次构建闭包里节点的 host_tools。
pub fn check(product: &Target, order: &[&Target]) -> Result<()> {
    let mut wanted: BTreeMap<String, ToolSpec> = BTreeMap::new();
    for tool in &product.profile.tools {
        add_tool(&mut wanted, tool);
    }
    for node in order {
        for tool in &node.host_tools {
            add_tool(&mut wanted, tool);
        }
    }

    let mut missing = Vec::new();
    for tool in wanted.values() {
        if !on_path(&tool.name) {
            missing.push(tool.name.clone());
            continue;
        }
        if tool.min.is_empty() {
            continue;
        }
        let current = probe_version(tool)?;
        if compare_versions(&current, &tool.min) < 0 {
            bail!("需要 {} >= {}，当前 {}", tool.name, tool.min, current);
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    bail!("缺少构建工具: {}", missing.join(", "))
}

/// 并集按名字取最严的一条：有 min 的优先，两条都有 min 时留更高的。
fn add_tool(wanted: &mut BTreeMap<String, ToolSpec>, tool: &ToolSpec) {
    let Some(old) = wanted.get(&tool.name) else {
        wanted.insert(tool.name.clone(), tool.clone());
        return;
    };
    let stricter =
        !tool.min.is_empty() && (old.min.is_empty() || compare_versions(&tool.min, &old.min) > 0);
    if stricter {
        wanted.insert(tool.name.clone(), tool.clone());
    }
}

fn probe_version(tool: &ToolSpec) -> Result<String> {
    let output = Command::new(&tool.name)
        .args(&tool.probe)
        .env("LC_ALL", "C")
        .output()
        .with_context(|| format!("跑不起来 {} 的版本探测", tool.name))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    find_version(&text).ok_or_else(|| {
        anyhow::anyhow!(
            "拿不到 {} 版本（命令: {} {}）",
            tool.name,
            tool.name,
            tool.probe.join(" ")
        )
    })
}

fn find_version(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let begin = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        let candidate = text[begin..index].trim_end_matches('.');
        if candidate.contains('.') {
            return Some(candidate.to_owned());
        }
    }
    None
}

fn compare_versions(current: &str, required: &str) -> i32 {
    let current = version_parts(current);
    let required = version_parts(required);
    let len = current.len().max(required.len());
    for index in 0..len {
        let left = *current.get(index).unwrap_or(&0);
        let right = *required.get(index).unwrap_or(&0);
        if left != right {
            return if left < right { -1 } else { 1 };
        }
    }
    0
}

fn version_parts(text: &str) -> Vec<u64> {
    text.split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().unwrap_or(0)
        })
        .collect()
}

fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        candidate.is_file()
            && candidate
                .metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_the_first_dotted_number() {
        assert_eq!(
            find_version("gcc (GCC) 15.3.1 20260722").as_deref(),
            Some("15.3.1")
        );
        assert_eq!(
            find_version("NASM version 2.16.03").as_deref(),
            Some("2.16.03")
        );
        assert_eq!(find_version("v5.38.2").as_deref(), Some("5.38.2"));
        assert_eq!(find_version("no version here"), None);
    }

    fn spec(name: &str, min: &str) -> ToolSpec {
        ToolSpec {
            name: name.to_owned(),
            min: min.to_owned(),
            probe: vec!["--version".to_owned()],
        }
    }

    #[test]
    fn union_keeps_the_strictest_requirement() {
        let mut wanted = BTreeMap::new();
        add_tool(&mut wanted, &spec("nasm", "2.13"));
        add_tool(&mut wanted, &spec("nasm", ""));
        assert_eq!(wanted["nasm"].min, "2.13");

        let mut wanted = BTreeMap::new();
        add_tool(&mut wanted, &spec("nasm", ""));
        add_tool(&mut wanted, &spec("nasm", "2.13"));
        assert_eq!(wanted["nasm"].min, "2.13");

        let mut wanted = BTreeMap::new();
        add_tool(&mut wanted, &spec("gcc", "9"));
        add_tool(&mut wanted, &spec("gcc", "11"));
        assert_eq!(wanted["gcc"].min, "11");
    }

    #[test]
    fn versions_compare_by_numeric_parts() {
        assert!(compare_versions("15.3.1", "11") > 0);
        assert!(compare_versions("9.4", "11") < 0);
        assert_eq!(compare_versions("4.0", "4.0.0"), 0);
        assert!(compare_versions("4.10", "4.9") > 0);
    }
}
