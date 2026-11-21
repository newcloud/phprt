use std::collections::VecDeque;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result, bail};

use phprt_download::{GitHubProxy, HostGate, Options, fetch_text, host_bucket};
use phprt_ui::{Mode, Tracker};

use crate::catalog::Catalog;
use crate::layout;

/// 哪条命令在问；决定标题和提问文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Install,
    Update,
    Add,
    Remove,
}

/// 本次闭包里需要检查的一个外部组件。
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
    /// 这次命令真的会下载或编译；不构建时不问。
    pub will_build: bool,
    /// `ask_before_online_update` 打开，且没有被 `--offline` / `--lock` / `--no-refresh` 跳过。
    pub enabled: bool,
    /// `--refresh`：不问，直接取；失败按错误处理。
    pub force: bool,
    pub default_yes: bool,
    pub mode: Mode,
    pub jobs: usize,
    pub host_jobs: usize,
    pub proxy: Option<String>,
    pub github_proxy: GitHubProxy,
    pub root: &'a Path,
    pub catalog: &'a Catalog,
}

pub struct Outcome {
    pub changed: bool,
}

#[derive(Debug)]
struct Change {
    old: String,
    new: String,
    changed: bool,
}

/// 问一次是否在线取 catalog 更新；取完返回 `changed = true`，调用方重跑一遍命令。
pub fn refresh(request: &Request<'_>) -> Result<Outcome> {
    if !request.force && (!request.enabled || !request.will_build) {
        return Ok(Outcome { changed: false });
    }
    if request.items.is_empty() {
        if request.force {
            println!("没有需要在线获取的组件，使用本地 catalog。");
        }
        return Ok(Outcome { changed: false });
    }

    let Some(base) = base_url() else {
        if request.force {
            bail!(
                "--refresh 需要 catalog 发布地址；设置 PHPRT_CATALOG_URL 或使用带内置地址的发行包"
            );
        }
        println!("注意: 没有配置 catalog 发布地址（PHPRT_CATALOG_URL），跳过在线获取");
        return Ok(Outcome { changed: false });
    };

    let proceed = if request.force {
        true
    } else if !std::io::stdin().is_terminal() {
        println!("非交互环境，跳过获取，使用本地");
        return Ok(Outcome { changed: false });
    } else {
        ask(request)?
    };
    if !proceed {
        println!("不获取，使用本地版本继续。");
        return Ok(Outcome { changed: false });
    }

    fetch_all(request, &base)
}

fn base_url() -> Option<String> {
    std::env::var("PHPRT_CATALOG_URL")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            option_env!("PHPRT_CATALOG_URL")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .map(|value| value.trim_end_matches('/').to_owned())
}

fn ask(request: &Request<'_>) -> Result<bool> {
    print_list(request);
    let default = if request.default_yes { "y" } else { "n" };
    println!("{}(y/n，回车默认 {default})", question(request.kind));
    println!("只获取这些组件的版本清单，不下载源码。");
    std::io::stdout().flush()?;

    loop {
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 {
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
        Kind::Install => "安装前是否在线获取最新补丁版本？",
        Kind::Update => "升级前是否在线获取最新补丁版本？",
        Kind::Add => "加入后会立即编译。是否先获取最新补丁版本？",
        Kind::Remove => "是否顺便获取剩余组件的最新补丁？",
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

fn fetch_all(request: &Request<'_>, base: &str) -> Result<Outcome> {
    let dir = layout::catalog_dir(request.root).join("components");
    fs::create_dir_all(&dir).with_context(|| format!("建不了 {}", dir.display()))?;

    let tags: Vec<String> = request
        .items
        .iter()
        .map(|item| format!("[catalog] {} {}", item.name, item.line))
        .collect();
    println!("正在获取 catalog 更新（{} 个组件）：", request.items.len());
    let ui = Tracker::new(tags, request.mode);
    let gate = HostGate::new(request.host_jobs);
    let options = Options {
        jobs: request.jobs,
        host_jobs: request.host_jobs,
        connections: 4,
        timeout: 10,
        proxy: request.proxy.clone(),
        github_proxy: request.github_proxy.clone(),
        offline: false,
        ui: request.mode,
    };
    let queue = Mutex::new((0..request.items.len()).collect::<VecDeque<_>>());
    let results: Mutex<Vec<Option<Result<Change, String>>>> =
        Mutex::new((0..request.items.len()).map(|_| None).collect());
    let jobs = request.jobs.max(1).min(request.items.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let index = queue.lock().expect("catalog 获取队列锁").pop_front();
                    let Some(index) = index else {
                        break;
                    };
                    let item = &request.items[index];
                    let url = format!("{base}/latest/components/{}.toml", item.name);
                    ui.start(index, "获取中", None);
                    let host = host_bucket(&url);
                    gate.acquire(&host);
                    let outcome = fetch_one(item, &url, &options, request.catalog, &dir);
                    gate.release(&host);
                    match &outcome {
                        Ok(change) => ui.finish(
                            index,
                            "✓",
                            &if change.changed {
                                if change.old.is_empty() {
                                    format!("新增 {}", change.new)
                                } else {
                                    format!("{} → {}", change.old, change.new)
                                }
                            } else {
                                format!("已是最新 {}", change.new)
                            },
                        ),
                        Err(reason) => ui.finish(
                            index,
                            "✗",
                            &format!("获取失败：{reason}，继续用本地 {}", item.local),
                        ),
                    }
                    results.lock().expect("catalog 结果锁")[index] = Some(outcome);
                }
            });
        }
    });
    ui.clear();

    let results = results.into_inner().expect("catalog 结果锁");
    let failures = results
        .iter()
        .filter(|result| matches!(result, Some(Err(_))))
        .count();
    let mut changed = false;
    for change in results.into_iter().flatten().flatten() {
        changed |= change.changed;
    }

    if request.force && failures > 0 {
        bail!(
            "catalog 刷新失败：{failures} 个组件没有取到；修好后重试，或去掉 --refresh 用本地数据"
        );
    }
    if failures > 0 {
        if changed {
            // 成功的组件已经换成新数据，stamp 要写这批数据的时间；失败的行只说明
            // 它们继续用本地版本，不能拿同一个 stamp 当成它们的数据时间。
            println!("{failures} 个组件没有取到，继续用本地版本。");
        } else {
            let stamp_path = layout::catalog_dir(request.root).join("stamp");
            let stamp = fs::read_to_string(&stamp_path)
                .ok()
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty());
            match stamp {
                Some(stamp) => {
                    println!("{failures} 个组件没有取到，继续使用本地数据（{stamp}）。")
                }
                None => println!("{failures} 个组件没有取到，继续使用本地数据。"),
            }
        }
    }

    if changed {
        let stamp = crate::catalog_publish::utc_now();
        let stamp_path = layout::catalog_dir(request.root).join("stamp");
        fs::write(&stamp_path, format!("{stamp}\n"))
            .with_context(|| format!("写不了 {}", stamp_path.display()))?;
        println!("catalog 已更新，继续下载并构建。");
    } else if failures > 0 {
        println!("没有取到新数据，继续。");
    } else {
        println!("没有发现新补丁，继续。");
    }
    Ok(Outcome { changed })
}

fn fetch_one(
    item: &Item,
    url: &str,
    options: &Options,
    catalog: &Catalog,
    dir: &Path,
) -> Result<Change, String> {
    crate::catalog_publish::check_component_name(&item.name).map_err(|err| err.to_string())?;
    let (_, text) = fetch_text(url, options).map_err(|err| format!("{err:#}"))?;
    apply_shard(item, url, &text, catalog, dir)
}

fn apply_shard(
    item: &Item,
    url: &str,
    text: &str,
    catalog: &Catalog,
    dir: &Path,
) -> Result<Change, String> {
    let shard = Catalog::parse(text, url).map_err(|err| format!("分片不合法: {err:#}"))?;
    if shard.components.len() != 1 || !shard.components.contains_key(&item.name) {
        return Err(format!("分片内容和组件名 {} 不一致", item.name));
    }
    let lines = &shard.components[&item.name];
    for (line, artifacts) in lines {
        for artifact in artifacts {
            if artifact.url.trim().is_empty() || artifact.sha256.trim().is_empty() {
                return Err(format!(
                    "{}.{line}.{} 缺少 url 或 sha256",
                    item.name, artifact.version
                ));
            }
        }
    }
    let artifacts = lines
        .get(&item.line)
        .ok_or_else(|| format!("分片里没有 {} {} 线", item.name, item.line))?;
    let new = artifacts
        .iter()
        .max_by(|left, right| phprt_upstream::compare(&left.version, &right.version))
        .map(|artifact| artifact.version.clone())
        .ok_or_else(|| format!("分片里 {} {} 线没有补丁", item.name, item.line))?;

    if !item.local.is_empty()
        && phprt_upstream::compare(&new, &item.local) == std::cmp::Ordering::Less
    {
        return Ok(Change {
            old: item.local.clone(),
            new: item.local.clone(),
            changed: false,
        });
    }

    // 写盘前先确认整片和本地基线不冲突，避免下次加载时报错。
    for (line, artifacts) in lines {
        for artifact in artifacts {
            if let Some(existing) = catalog.artifact_for(&item.name, line, &artifact.version) {
                if existing.url != artifact.url {
                    return Err(format!(
                        "和本地 catalog 冲突：{}.{line}.{} 的 url 不一致",
                        item.name, artifact.version
                    ));
                }
                if !existing.sha256.trim().is_empty()
                    && !existing.sha256.eq_ignore_ascii_case(&artifact.sha256)
                {
                    return Err(format!(
                        "和本地 catalog 冲突：{}.{line}.{} 的 sha256 不一致",
                        item.name, artifact.version
                    ));
                }
            }
        }
    }

    let path = dir.join(format!("{}.toml", item.name));
    let tmp = dir.join(format!(".{}.tmp-{}", item.name, std::process::id()));
    fs::write(&tmp, text).map_err(|err| format!("写不了 {}: {err}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|err| format!("写不了 {}: {err}", path.display()))?;

    // 版本号没变、但补上了缺失的哈希，也要重跑一遍，让下载用上新哈希。
    // 只看本次绑定的线；其它线的新补丁已经写盘，但不影响这次方案。
    let mut changed = new != item.local;
    for artifact in artifacts {
        match catalog.artifact_for(&item.name, &item.line, &artifact.version) {
            None => changed = true,
            Some(existing)
                if existing.sha256.trim().is_empty() && !artifact.sha256.trim().is_empty() =>
            {
                changed = true
            }
            _ => {}
        }
    }
    Ok(Change {
        old: item.local.clone(),
        new,
        changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("phprt-remote-{tag}-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn base() -> Catalog {
        Catalog::parse(
            r#"
[php]
"8.5" = [ { version = "8.5.12", url = "https://example.com/8.5.12.tar.gz" } ]
"#,
            "fixture",
        )
        .unwrap()
    }

    fn item() -> Item {
        Item {
            name: "php".to_owned(),
            line: "8.5".to_owned(),
            installed: "8.5.12".to_owned(),
            local: "8.5.12".to_owned(),
        }
    }

    #[test]
    fn shard_is_validated_and_written() {
        let dir = temp_dir("write");
        let change = apply_shard(
            &item(),
            "https://catalog.example/latest/components/php.toml",
            r#"
[php]
"8.5" = [
    { version = "8.5.12", url = "https://example.com/8.5.12.tar.gz", sha256 = "aa" },
    { version = "8.5.13", url = "https://example.com/8.5.13.tar.gz", sha256 = "bb" },
]
"#,
            &base(),
            &dir,
        )
        .unwrap();
        assert!(change.changed);
        assert_eq!(change.new, "8.5.13");
        assert!(dir.join("php.toml").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_hash_or_wrong_component_is_rejected() {
        let dir = temp_dir("bad");
        let err = apply_shard(
            &item(),
            "https://catalog.example/latest/components/php.toml",
            r#"
[php]
"8.5" = [ { version = "8.5.13", url = "https://example.com/8.5.13.tar.gz" } ]
"#,
            &base(),
            &dir,
        )
        .unwrap_err();
        assert!(err.contains("缺少 url 或 sha256"), "{err}");

        let err = apply_shard(
            &item(),
            "https://catalog.example/latest/components/php.toml",
            r#"
[openssl]
"3.5" = [ { version = "3.5.10", url = "https://example.com/3.5.10.tar.gz", sha256 = "cc" } ]
"#,
            &base(),
            &dir,
        )
        .unwrap_err();
        assert!(err.contains("组件名"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn conflict_and_downgrade_keep_the_local_shard() {
        let dir = temp_dir("conflict");
        let err = apply_shard(
            &item(),
            "https://catalog.example/latest/components/php.toml",
            r#"
[php]
"8.5" = [ { version = "8.5.12", url = "https://other.example/8.5.12.tar.gz", sha256 = "aa" } ]
"#,
            &base(),
            &dir,
        )
        .unwrap_err();
        assert!(err.contains("url 不一致"), "{err}");

        let change = apply_shard(
            &item(),
            "https://catalog.example/latest/components/php.toml",
            r#"
[php]
"8.5" = [ { version = "8.5.11", url = "https://example.com/8.5.11.tar.gz", sha256 = "dd" } ]
"#,
            &base(),
            &dir,
        )
        .unwrap();
        assert!(!change.changed);
        assert!(!dir.join("php.toml").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
