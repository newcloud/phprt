//! install / update / add / remove 前的上游补丁检查。
//!
//! 用户只关心本次方案用到的组件主线：只查这些线有没有更大的补丁，不跨线、
//! 不新增线、不扫无关组件。查到后写回 `recipes/catalog.toml`（写前备份），
//! 调用方重新算方案再下载、构建。

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::catalog::Catalog;

/// 哪条命令在检查；决定标题和提问文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Install,
    Update,
    Add,
    Remove,
}

/// 本次闭包里需要检查的一条主线。
#[derive(Debug, Clone)]
pub struct Item {
    pub name: String,
    pub line: String,
    pub installed: String,
    pub local: String,
}

pub struct Request<'a> {
    pub kind: Kind,
    pub product: &'a str,
    pub series: &'a str,
    pub add_name: Option<&'a str>,
    pub items: Vec<Item>,
    /// 这次命令真的会下载或编译；不构建时不检查。
    pub will_build: bool,
    /// `ask_before_online_update` 打开，且没有被 `--offline` / `--lock` / `--no-refresh` 跳过。
    pub enabled: bool,
    /// `--refresh`：不问，直接查；失败按错误处理。
    pub force: bool,
    pub default_yes: bool,
    pub recipes: &'a Path,
    pub cache: &'a Path,
    pub backup_dir: &'a Path,
    pub download: phprt_download::Options,
}

pub struct Outcome {
    pub changed: bool,
}

/// 问一次是否检查上游补丁；取到新补丁返回 `changed = true`，调用方重跑命令。
pub fn check(request: Request<'_>) -> Result<Outcome> {
    if !request.force && (!request.enabled || !request.will_build) {
        return Ok(Outcome { changed: false });
    }
    if request.items.is_empty() {
        if request.force {
            println!("方案里没有需要检查的组件。");
        }
        return Ok(Outcome { changed: false });
    }
    if !request.force && !std::io::stdin().is_terminal() {
        println!("非交互环境，跳过上游补丁检查，使用本地 catalog");
        return Ok(Outcome { changed: false });
    }
    if !request.force && !ask(&request)? {
        println!("不检查，使用本地版本继续。");
        return Ok(Outcome { changed: false });
    }

    let mut components: Vec<String> = Vec::new();
    let mut lines: Vec<(String, String)> = Vec::new();
    for item in &request.items {
        if !components.contains(&item.name) {
            components.push(item.name.clone());
        }
        lines.push((item.name.clone(), item.line.clone()));
    }

    let report = phprt_upstream::run(phprt_upstream::Options {
        recipes: request.recipes.to_path_buf(),
        cache: request.cache.to_path_buf(),
        backup_dir: request.backup_dir.to_path_buf(),
        download: request.download,
        dry_run: false,
        fill_sha256: false,
        components,
        lines,
        validate: Box::new(|path| Catalog::load(path).map(|_| ())),
    })?;
    print!("{}", report.text());
    let changed = report.changed();
    if changed {
        println!("catalog 已更新，继续。");
    }
    if request.force && report.partial_failure {
        bail!("上游补丁检查有失败项；--refresh 要求全部成功");
    }
    Ok(Outcome { changed })
}

fn ask(request: &Request<'_>) -> Result<bool> {
    print_list(request);
    let default = if request.default_yes { "y" } else { "n" };
    print!("{} (y/n，回车默认 {default}) ", question(request.kind));
    std::io::stdout().flush()?;
    let mut line = String::new();
    loop {
        line.clear();
        if std::io::stdin().read_line(&mut line)? == 0 {
            // 交互式 EOF 当 n，避免 CI 意外联网。
            return Ok(false);
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(request.default_yes),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("请输入 y 或 n。"),
        }
    }
}

fn question(kind: Kind) -> &'static str {
    match kind {
        Kind::Install => "安装前是否检查上游这条主线的最新补丁？",
        Kind::Update => "升级前是否检查上游这条主线的最新补丁？",
        Kind::Add => "加入后会立即编译。是否先检查上游最新补丁？",
        Kind::Remove => "是否顺便检查剩余组件的上游最新补丁？",
    }
}

fn print_list(request: &Request<'_>) {
    match request.kind {
        Kind::Update => {
            if request.items.iter().any(|item| !item.installed.is_empty()) {
                println!("{} {} 当前安装：", request.product, request.series);
            } else {
                println!(
                    "准备安装 {} {}，会用到这些组件：",
                    request.product, request.series
                );
            }
            let rows: Vec<(String, String)> = request
                .items
                .iter()
                .map(|item| {
                    let first = if item.installed.is_empty() {
                        format!("{} {}", item.name, item.line)
                    } else {
                        format!("{} {}", item.name, item.installed)
                    };
                    let mut status = format!("本地 {}", item.local);
                    if !item.installed.is_empty() {
                        status.push_str("  ");
                        status.push(
                            match phprt_upstream::compare(&item.local, &item.installed) {
                                std::cmp::Ordering::Greater => '↑',
                                std::cmp::Ordering::Equal => '=',
                                std::cmp::Ordering::Less => '↓',
                            },
                        );
                    }
                    (first, status)
                })
                .collect();
            let width = rows.iter().map(|(first, _)| first.len()).max().unwrap_or(0);
            for (first, status) in rows {
                println!("  {first:<width$}  {status}");
            }
        }
        Kind::Install | Kind::Add | Kind::Remove => {
            println!("{}", header(request));
            let rows: Vec<(String, String)> = request
                .items
                .iter()
                .map(|item| {
                    let first = format!("{} {}", item.name, item.line);
                    let status = if item.installed.is_empty() {
                        format!("本地 {}", item.local)
                    } else if item.installed == item.local {
                        format!("已装 {}", item.installed)
                    } else {
                        format!("已装 {}，本地 {}", item.installed, item.local)
                    };
                    (first, status)
                })
                .collect();
            let width = rows.iter().map(|(first, _)| first.len()).max().unwrap_or(0);
            for (first, status) in rows {
                println!("  {first:<width$}  {status}");
            }
        }
    }
    println!();
    println!("只查上面这些主线；有新补丁会下载源码包计算 sha256，写回 recipes/catalog.toml。");
    println!();
}

fn header(request: &Request<'_>) -> String {
    match request.kind {
        Kind::Install => format!(
            "准备安装 {} {}，会用到这些组件：",
            request.product, request.series
        ),
        Kind::Update => format!("{} {} 当前安装：", request.product, request.series),
        Kind::Add => format!(
            "准备加入 {}，会用到这些组件：",
            request.add_name.unwrap_or("组件")
        ),
        Kind::Remove => "移除后需要重新构建，会用到这些组件：".to_owned(),
    }
}

/// 便于 main 组装请求时复用。
pub fn backup_dir(root: &Path) -> PathBuf {
    match crate::layout::backup_dir_name() {
        Some(name) => crate::layout::resolve(root, name),
        None => crate::layout::internal_dir(root).join("backup"),
    }
}
