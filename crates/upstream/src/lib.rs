//! catalog 自动更新：查上游最新补丁，写进 recipes/catalog.toml。
//!
//! 方案和边界见 docs/CATALOG-UPDATE.md。入口是 `run`；`src/main.rs` 只做参数解析。

mod artifact;
mod catalog;
mod report;
mod script;
mod source;
mod version;

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result, bail};
use toml_edit::DocumentMut;

pub use report::{Report, Row, Status};
pub use version::{belongs_to_line, compare, is_unstable};

use phprt_download::{HostGate, Outcome, host_bucket};
use phprt_source::Source;
use phprt_ui::Tracker;

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
/// 查询和下载并行执行，所以要 `Send + Sync`。
pub type Validate = dyn Fn(&Path) -> Result<()> + Send + Sync;

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
    /// 回填已有条目缺失的 sha256；默认 false，不加 `--fill-sha256` 不下载旧包。
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

    // 先把任务列出来：每个组件一行，查询和逐条检查都在这行上推进。
    // 终端里进度框会先显示全部「等待」，查到哪个组件就切到「查询中 / 检查 <线>」；
    // 非终端（管道、CI）退化成一行一个组件的开始和结束，不会长时间没有输出。
    let ui = Tracker::new(
        components
            .iter()
            .map(|component| format!("[catalog] {component}"))
            .collect(),
        options.download.ui,
    );

    // 查询和下载用同一套并发限制：同时 10 个组件，同一站点最多 2 个。
    let gate = HostGate::new(options.download.host_jobs);
    let queue = Mutex::new((0..components.len()).collect::<VecDeque<_>>());
    let results: Mutex<Vec<Option<Vec<Step>>>> =
        Mutex::new((0..components.len()).map(|_| None).collect());
    let jobs = options.download.jobs.max(1).min(components.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let next = queue.lock().expect("查询队列锁").pop_front();
                    let Some(index) = next else {
                        break;
                    };
                    let steps =
                        check_component(&options, &doc, &ui, &gate, index, &components[index]);
                    results.lock().expect("查询结果锁")[index] = Some(steps);
                }
            });
        }
    });

    // 查询阶段的进度框先收掉，下载阶段会建自己的进度框。
    drop(ui);

    // 按组件原顺序拼回结果，报告顺序和顺序执行时一致。
    let mut steps: Vec<Step> = results
        .into_inner()
        .expect("查询结果锁")
        .into_iter()
        .flatten()
        .flatten()
        .collect();

    if options.fill_sha256 {
        plan_fill(&options, &doc, &mut steps);
    }

    apply_steps(&options, &mut doc, &mut report, steps)?;

    if report.changed() && !options.dry_run {
        write_back(&options, &catalog_path, &doc)?;
    }
    Ok(report)
}

/// 判断一个组件：加载脚本、查上游、逐条线检查。返回这个组件的步骤。
/// 查询也用 `HostGate` 限流；拿不到站点名额时进度行停在「等待」。
fn check_component(
    options: &Options,
    doc: &DocumentMut,
    ui: &Tracker,
    gate: &HostGate,
    index: usize,
    component: &str,
) -> Vec<Step> {
    let mut steps = Vec::new();
    let lines = match catalog::lines(doc, component) {
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
                ui.finish(index, "✗", "catalog 里没有这个组件");
            } else {
                ui.finish(index, "-", "catalog 里没有这个组件");
            }
            return steps;
        }
    };
    let script = match script::Script::load(&options.recipes, component) {
        Ok(script) => script,
        Err(err) => {
            // 全量 sweep 记「跳过」；明确点名要更新、又找不到脚本才算失败。
            let status = if options.components.is_empty() {
                Status::Skipped
            } else {
                Status::Failed
            };
            let note = format!("没有 upstream 脚本：{err:#}");
            steps.push(done(component, "", status, "", "", note.clone()));
            ui.finish(
                index,
                if status == Status::Failed { "✗" } else { "-" },
                &note,
            );
            return steps;
        }
    };

    let host = host_bucket(&script.url);
    gate.acquire(&host);
    ui.start(index, "查询中", None);

    // 执行原则：一次查询覆盖全部线；URL 带 {line} 的源按线查（源形态决定）。
    let per_line = script.url.contains("{line}");
    let fetched_once = if per_line {
        None
    } else {
        match source::query(&script, None, &options.download) {
            Ok(list) => Some(list),
            Err(err) => {
                let note = format!("查询失败: {err:#}");
                steps.push(done(component, "", Status::Failed, "", "", note.clone()));
                gate.release(&host);
                ui.finish(index, "✗", &note);
                return steps;
            }
        }
    };

    let before = steps.len();
    for line in &lines {
        let verb = if per_line {
            format!("查询 {line}")
        } else {
            format!("检查 {line}")
        };
        ui.set_status(index, &verb);
        steps.push(plan_line(
            options,
            doc,
            component,
            line,
            &script,
            fetched_once.as_ref(),
        ));
    }
    gate.release(&host);

    let (mark, text) = summarize(&steps[before..]);
    ui.finish(index, mark, &text);
    steps
}

/// 一个组件这一轮的结果摘要，用于进度行收尾：`✓ 新增 1 条线，无更新 3 条线`。
fn summarize(steps: &[Step]) -> (&'static str, String) {
    let mut new = 0;
    let mut filled = 0;
    let mut current = 0;
    let mut no_candidate = 0;
    let mut skipped = 0;
    let mut failed = 0;
    for step in steps {
        let status = match step {
            Step::Done(row) => row.status,
            Step::Download(item) => item.status,
        };
        match status {
            Status::NewPatch => new += 1,
            Status::Filled => filled += 1,
            Status::Current => current += 1,
            Status::NoCandidate => no_candidate += 1,
            Status::Skipped => skipped += 1,
            Status::Failed => failed += 1,
        }
    }
    let mut parts = Vec::new();
    for (count, label) in [
        (new, "新增"),
        (filled, "回填"),
        (current, "无更新"),
        (no_candidate, "无候选"),
        (skipped, "跳过"),
        (failed, "失败"),
    ] {
        if count > 0 {
            parts.push(format!("{label} {count} 条线"));
        }
    }
    let text = if parts.is_empty() {
        "完成".to_owned()
    } else {
        parts.join("，")
    };
    (if failed > 0 { "✗" } else { "✓" }, text)
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
            Status::NoCandidate,
            "",
            "",
            "上游返回里没有属于这条线的版本（接口可能只返回最近若干条）".to_owned(),
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

/// 回填已有条目缺失的 sha256；已有值的条目不动。只在 `--fill-sha256` 时调用。
/// 传了组件名时只处理这些组件（和更新范围一致）。
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
        // 和 install 的项目录一致：<缓存根>/<组件名>/<文件名>，同一个包能互相命中。
        dest: options.cache.join(component).join(name),
    })
}

/// 同一批里两个不同地址落到同一个缓存文件名时直接失败：否则两个任务写同一个
/// 文件，最后都会对同一个文件算哈希，把其中一条的 sha256 写错。install 的清单
/// 生成也有同样的检查（src/sources.rs）。
fn check_cache_names(pending: &[(usize, &Pending)]) -> Result<()> {
    let mut seen: BTreeMap<&Path, (&str, &str)> = BTreeMap::new();
    for (_, item) in pending {
        if let Some((other_component, other_url)) =
            seen.insert(item.dest.as_path(), (&item.component, &item.url))
            && other_url != item.url
        {
            bail!(
                "缓存文件名冲突: {}（{other_component} 用 {other_url}，{} 用 {}）",
                item.dest.display(),
                item.component,
                item.url
            );
        }
    }
    Ok(())
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

    check_cache_names(&pending)?;

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
        let results = phprt_download::run_outcomes(&sources, &options.cache, &options.download)?;
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
            item.status,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(component: &str, url: &str) -> Pending {
        let name = phprt_download::archive_name(url).expect("URL 有文件名");
        Pending {
            component: component.to_owned(),
            line: "1.0".to_owned(),
            status: Status::NewPatch,
            version: "1.0.0".to_owned(),
            url: url.to_owned(),
            dest: PathBuf::from("/cache").join(name),
        }
    }

    #[test]
    fn different_urls_with_the_same_cache_name_are_rejected() {
        // GitHub tag 归档的文件名是裸的 <tag>.tar.gz，两个仓库同 tag 就会撞名。
        let items = [
            pending(
                "avif",
                "https://github.com/AOMediaCodec/libavif/archive/refs/tags/v1.4.2.tar.gz",
            ),
            pending(
                "other",
                "https://github.com/someone/other/archive/refs/tags/v1.4.2.tar.gz",
            ),
        ];
        let steps: Vec<(usize, &Pending)> = items.iter().enumerate().collect();
        let err = check_cache_names(&steps).unwrap_err();
        assert!(err.to_string().contains("缓存文件名冲突"), "{err}");
    }

    #[test]
    fn the_same_url_shares_one_cache_file() {
        let items = [
            pending("avif", "https://x/v1.4.2.tar.gz"),
            pending("avif", "https://x/v1.4.2.tar.gz"),
        ];
        let steps: Vec<(usize, &Pending)> = items.iter().enumerate().collect();
        assert!(check_cache_names(&steps).is_ok());
    }
}
