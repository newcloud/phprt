//! catalog 自动更新：查上游最新补丁，写进 recipes/catalog.toml。
//!
//! 方案和边界见 docs/CATALOG-UPDATE.md。入口是 `run`；`src/main.rs` 只做参数解析。

mod artifact;
mod catalog;
mod report;
mod script;
mod source;
mod version;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::DocumentMut;

pub use report::{Report, Row, Status};
pub use version::{belongs_to_line, compare, is_unstable};

use phprt_download::Outcome;
use phprt_source::Source;

/// 本次要下载的一条；下载成功后再写进 catalog。
struct Pending {
    component: String,
    line: String,
    status: Status,
    version: String,
    url: String,
    dest: PathBuf,
}

/// 按组件 / 线的处理顺序排好的步骤：已有结论的直接出报告，要下载的等整批跑完。
enum Step {
    Done(Row),
    Download(Pending),
}

/// 写回前对临时 catalog 的校验函数（main 注入 `Catalog::load`）。
pub type Validate = dyn Fn(&Path) -> Result<()>;

/// 一次 catalog 更新的全部输入。
pub struct Options {
    /// recipes 目录（catalog.toml 和 upstream/ 都在它下面）。
    pub recipes: PathBuf,
    /// 下载缓存目录（和 install 同一个缓存）。
    pub cache: PathBuf,
    /// catalog 备份目录（layout 已经按配置解析过）。
    pub backup_dir: PathBuf,
    /// 下载器设置（候选地址 / 代理 / 超时）。
    pub download: phprt_download::Options,
    /// 只打印计划，不写文件。
    pub dry_run: bool,
    /// 回填已有条目缺失的 sha256。
    pub fill_sha256: bool,
    /// 只更新这些组件；空表示全部。
    pub components: Vec<String>,
    /// 写回前对临时文件做和加载时一样的校验（main 传 Catalog::load）。
    pub validate: Box<Validate>,
}

pub fn run(options: Options) -> Result<Report> {
    let catalog_path = options.recipes.join("catalog.toml");
    let mut doc = catalog::read(&catalog_path)?;
    let mut report = Report::default();

    let components = if options.components.is_empty() {
        catalog::components(&doc)
    } else {
        options.components.clone()
    };

    // 先做完全部判断，再一批下载，最后按原顺序写回：需要下载的文件共享下载器的
    // 并发队列（10 个文件 / 每站点 2 个），不逐条串行。
    let mut steps = Vec::new();

    for component in &components {
        let lines = match catalog::lines(&doc, component) {
            Some(lines) => lines,
            None => {
                if !options.components.is_empty() {
                    steps.push(done(
                        component,
                        "",
                        Status::Failed,
                        "",
                        "",
                        "catalog 里没有这个组件".to_owned(),
                    ));
                }
                continue;
            }
        };
        let script = match script::Script::load(&options.recipes, component) {
            Ok(script) => script,
            Err(err) => {
                // 回填不需要脚本；只有明确点名要更新、又找不到脚本才算失败。
                let status = if options.components.is_empty() || options.fill_sha256 {
                    Status::Skipped
                } else {
                    Status::Failed
                };
                steps.push(done(
                    component,
                    "",
                    status,
                    "",
                    "",
                    format!("没有 upstream 脚本：{err:#}"),
                ));
                continue;
            }
        };

        // 执行原则：一次查询覆盖全部线；URL 带 {line} 的源按线查（源形态决定）。
        let fetched_once = if script.url.contains("{line}") {
            None
        } else {
            match source::query(&script, None, &options.download) {
                Ok(list) => Some(list),
                Err(err) => {
                    steps.push(done(
                        component,
                        "",
                        Status::Failed,
                        "",
                        "",
                        format!("查询失败: {err:#}"),
                    ));
                    continue;
                }
            }
        };

        for line in &lines {
            steps.push(plan_line(
                &options,
                &doc,
                component,
                line,
                &script,
                fetched_once.as_ref(),
            ));
        }
    }

    if options.fill_sha256 {
        plan_fill(&options, &doc, &mut steps);
    }

    apply_steps(&options, &mut doc, &mut report, steps)?;

    if report.changed() && !options.dry_run {
        write_back(&options, &catalog_path, &doc)?;
    }
    Ok(report)
}

/// 一条线的判断：取线内最大候选，比当前最大补丁大才排进下载队列。
fn plan_line(
    options: &Options,
    doc: &DocumentMut,
    component: &str,
    line: &str,
    script: &script::Script,
    once: Option<&Vec<source::Candidate>>,
) -> Step {
    let candidates = match once {
        Some(list) => list.clone(),
        None => match source::query(script, Some(line), &options.download) {
            Ok(list) => list,
            Err(err) => {
                return done(
                    component,
                    line,
                    Status::Failed,
                    "",
                    "",
                    format!("查询失败: {err:#}"),
                );
            }
        },
    };
    let eligible: Vec<_> = candidates
        .into_iter()
        .filter(|c| !version::is_unstable(&c.version))
        .filter(|c| version::belongs_to_line(&c.version, line))
        .collect();
    let current = catalog::max_version(doc, component, line);
    let Some(best) = eligible
        .iter()
        .max_by(|a, b| version::compare(&a.version, &b.version))
    else {
        return done(
            component,
            line,
            Status::Current,
            "",
            "",
            "该线没有候选".to_owned(),
        );
    };
    if let Some(cur) = &current
        && version::compare(&best.version, cur) != std::cmp::Ordering::Greater
    {
        return done(
            component,
            line,
            Status::Current,
            "",
            "",
            format!("已是最新 {cur}"),
        );
    }

    // 先核对文件名版本，再探测可达性；dry-run 到此为止，不下载文件。
    // 版本和连接从同一行提取时（link 自带 version 组，如 sqlite）不核对：
    // 两者同源，文件名和版本不直接对应是源本身的编码，脚本已经声明了对应关系。
    if script.version.is_some() && !artifact::filename_matches(&best.url, &best.version) {
        return done(
            component,
            line,
            Status::Failed,
            &best.version,
            &best.url,
            "下载连接里的文件名版本号和提取出的版本不一致，不写入".to_owned(),
        );
    }
    if let Err(err) = phprt_download::final_url(&best.url, &options.download) {
        return done(
            component,
            line,
            Status::Failed,
            &best.version,
            &best.url,
            format!("连接不可达: {err:#}"),
        );
    }
    if options.dry_run {
        return done(
            component,
            line,
            Status::NewPatch,
            &best.version,
            &best.url,
            "dry-run：未写文件".to_owned(),
        );
    }
    download_step(
        options,
        component,
        line,
        Status::NewPatch,
        &best.version,
        &best.url,
    )
}

/// 回填已有条目缺失的 sha256；已有值的条目不动。
/// 传了组件名时只处理这些组件（和更新范围一致，docs/CATALOG-UPDATE.md 15）。
fn plan_fill(options: &Options, doc: &DocumentMut, steps: &mut Vec<Step>) {
    let missing = catalog::missing_sha256(doc);
    for (component, line, version, url) in missing {
        if !options.components.is_empty() && !options.components.contains(&component) {
            continue;
        }
        if options.dry_run {
            steps.push(done(
                &component,
                &line,
                Status::Filled,
                &version,
                "",
                "dry-run：未写文件".to_owned(),
            ));
            continue;
        }
        steps.push(download_step(
            options,
            &component,
            &line,
            Status::Filled,
            &version,
            &url,
        ));
    }
}

fn done(
    component: &str,
    line: &str,
    status: Status,
    version: &str,
    url: &str,
    note: String,
) -> Step {
    Step::Done(Row {
        component: component.to_owned(),
        line: line.to_owned(),
        status,
        version: version.to_owned(),
        url: url.to_owned(),
        note,
    })
}

fn download_step(
    options: &Options,
    component: &str,
    line: &str,
    status: Status,
    version: &str,
    url: &str,
) -> Step {
    let name = phprt_download::archive_name(url)
        .unwrap_or_else(|| format!("{component}-{version}-download"));
    Step::Download(Pending {
        component: component.to_owned(),
        line: line.to_owned(),
        status,
        version: version.to_owned(),
        url: url.to_owned(),
        dest: options.cache.join(name),
    })
}

/// 一批下载结束后，按判断顺序逐条算哈希、写进 catalog。
fn apply_steps(
    options: &Options,
    doc: &mut DocumentMut,
    report: &mut Report,
    steps: Vec<Step>,
) -> Result<()> {
    let pending: Vec<(usize, &Pending)> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| match step {
            Step::Download(item) => Some((index, item)),
            Step::Done(_) => None,
        })
        .collect();

    let mut outcomes: Vec<Option<Outcome>> = (0..steps.len()).map(|_| None).collect();
    if !pending.is_empty() {
        let sources: Vec<Source> = pending
            .iter()
            .map(|(_, item)| Source {
                label: "upstream".to_owned(),
                name: phprt_download::archive_name(&item.url)
                    .unwrap_or_else(|| format!("{}-{}-download", item.component, item.version)),
                version: item.version.clone(),
                url: item.url.clone(),
                sha256: String::new(),
                archive: item.dest.clone(),
                unpack: item.dest.clone(),
                unpack_display: String::new(),
            })
            .collect();
        let results = phprt_download::run_outcomes(&sources, &options.download)?;
        for ((index, _), outcome) in pending.iter().zip(results) {
            outcomes[*index] = Some(outcome);
        }
    }

    for (index, step) in steps.into_iter().enumerate() {
        match step {
            Step::Done(row) => report.push_row(row),
            Step::Download(item) => {
                let outcome = outcomes[index].take().expect("下载结果");
                apply_one(doc, report, &item, outcome);
            }
        }
    }
    Ok(())
}

fn apply_one(doc: &mut DocumentMut, report: &mut Report, item: &Pending, outcome: Outcome) {
    if let Outcome::Failed(reason) = outcome {
        report.push(
            &item.component,
            &item.line,
            Status::Failed,
            &item.version,
            &item.url,
            format!("下载失败: {reason}"),
        );
        return;
    }
    let digest = match phprt_download::sha256_file(&item.dest) {
        Ok(digest) => digest,
        Err(err) => {
            report.push(
                &item.component,
                &item.line,
                Status::Failed,
                &item.version,
                &item.url,
                format!("算 sha256 失败: {err:#}"),
            );
            return;
        }
    };
    if item.status == Status::Filled {
        match catalog::fill_sha256(doc, &item.component, &item.line, &item.version, &digest) {
            Ok(true) => report.push(
                &item.component,
                &item.line,
                Status::Filled,
                &item.version,
                "",
                String::new(),
            ),
            Ok(false) => {}
            Err(err) => report.push(
                &item.component,
                &item.line,
                Status::Failed,
                &item.version,
                "",
                format!("回填失败: {err:#}"),
            ),
        }
        return;
    }
    match catalog::append_patch(
        doc,
        &item.component,
        &item.line,
        &catalog::Patch {
            version: item.version.clone(),
            url: item.url.clone(),
            sha256: digest,
        },
    ) {
        Ok(true) => report.push(
            &item.component,
            &item.line,
            Status::NewPatch,
            &item.version,
            &item.url,
            String::new(),
        ),
        Ok(false) => report.push(
            &item.component,
            &item.line,
            Status::Current,
            &item.version,
            "",
            "catalog 里已有这个版本".to_owned(),
        ),
        Err(err) => report.push(
            &item.component,
            &item.line,
            Status::Failed,
            &item.version,
            "",
            format!("写 catalog 失败: {err:#}"),
        ),
    }
}

/// 备份一次 → 写临时文件 → 用 main 注入的校验复检 → 原子替换。
fn write_back(options: &Options, path: &Path, doc: &DocumentMut) -> Result<()> {
    let saved = catalog::backup(path, &options.backup_dir)?;
    let tmp = path.with_extension("toml.tmp");
    catalog::write(&tmp, doc).with_context(|| format!("写临时文件失败: {}", tmp.display()))?;
    if let Err(err) = (options.validate)(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(err.context(format!(
            "复检 {} 失败，保留原文件；备份在 {}",
            path.display(),
            saved.display()
        )));
    }
    fs::rename(&tmp, path).with_context(|| format!("替换不了 {}", path.display()))?;
    println!("已备份: {}", saved.display());
    println!("已写入: {}", path.display());
    Ok(())
}
