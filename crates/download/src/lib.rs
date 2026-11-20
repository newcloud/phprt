mod candidate;
mod store;
mod transfer;

pub use candidate::{GitHubProxy, candidates, is_github_release};
pub use store::{archive_name, sha256_file};

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

use anyhow::{Context, Result, bail};
use ureq::http::Method;

use phprt_source::Source;
use phprt_ui::{Mode, Tracker, human, tag};

#[derive(Debug, Clone)]
pub struct Options {
    pub jobs: usize,
    pub host_jobs: usize,
    pub connections: usize,
    pub timeout: u64,
    pub proxy: Option<String>,
    pub github_proxy: GitHubProxy,
    pub offline: bool,
    pub ui: Mode,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Done(u64),
    Skipped(u64),
    Failed(String),
}

fn finish(ui: &Tracker, index: usize, outcome: &Outcome) {
    match outcome {
        Outcome::Done(size) => ui.finish(index, "✓", &format!("完成（{}）", human(*size))),
        Outcome::Skipped(size) => ui.finish(index, "✓", &format!("已存在（{}）", human(*size))),
        Outcome::Failed(reason) => ui.finish(index, "✗", &format!("失败: {reason}")),
    }
}

pub fn run(sources: &[Source], opts: &Options) -> Result<()> {
    let results = run_outcomes(sources, opts)?;
    if sources.is_empty() {
        return Ok(());
    }
    let mut done = 0;
    let mut skipped = 0;
    let mut failed = Vec::new();
    for (index, outcome) in results.iter().enumerate() {
        match outcome {
            Outcome::Done(_) => done += 1,
            Outcome::Skipped(_) => skipped += 1,
            Outcome::Failed(reason) => failed.push((index, reason.clone())),
        }
    }

    if failed.is_empty() {
        eprintln!("下载完成: {done} 个，已存在 {skipped} 个");
        return Ok(());
    }

    eprintln!();
    eprintln!("下载失败 {} 个（文件保留，下次运行可继续）:", failed.len());
    for (index, reason) in &failed {
        let source = &sources[*index];
        eprintln!("  [{}] {} {}", source.label, source.name, source.version);
        eprintln!("    {}", source.url);
        eprintln!("    原因: {reason}");
        let candidates = candidate::candidates(&source.url, &opts.github_proxy);
        if candidates.len() > 1 {
            eprintln!("    镜像: {}", candidates[1]);
        }
    }
    bail!("{} 个源码包下载失败", failed.len())
}

/// 和 `run` 同一个下载队列，但把每个文件的结果按输入顺序交回调用方，
/// 单个文件失败不影响其它文件。catalog 更新用来一批下载多个组件。
pub fn run_outcomes(sources: &[Source], opts: &Options) -> Result<Vec<Outcome>> {
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let cache = sources[0]
        .archive
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&cache).with_context(|| format!("建不了 {}", cache.display()))?;

    eprintln!("源码缓存: {}", cache.display());
    eprintln!(
        "并发: {} 个文件（每站点 {}），单文件分片 {}（≥8 MB 才启用）",
        opts.jobs.max(1),
        opts.host_jobs.max(1),
        opts.connections.max(1)
    );
    if let Some(proxy) = &opts.proxy {
        eprintln!("代理: {proxy}");
    }
    if opts.offline {
        eprintln!("离线模式: 只使用缓存，不联网");
    }

    let tags = sources
        .iter()
        .map(|source| tag(&source.label, &source.name, &source.version))
        .collect();
    let ui = Tracker::new(tags, opts.ui);
    if !ui.is_plain() {
        // Ctrl-C 时把光标恢复，半截文件留给下次续传。
        let _ = ctrlc::set_handler(|| {
            eprintln!("\x1b[?25h");
            std::process::exit(130);
        });
    }
    let agents = transfer::agents(opts)?;
    let gate = HostGate::new(opts.host_jobs);
    let queue = Mutex::new((0..sources.len()).collect::<VecDeque<_>>());
    let results = Mutex::new(vec![None; sources.len()]);
    let jobs = opts.jobs.max(1).min(sources.len());

    std::thread::scope(|scope| {
        for _ in 0..jobs {
            scope.spawn(|| {
                loop {
                    let index = queue.lock().expect("下载队列锁").pop_front();
                    let Some(index) = index else {
                        break;
                    };
                    let source = &sources[index];
                    ui.start(index, "下载中", None);
                    let host = candidate::host_bucket(&source.url);
                    gate.acquire(&host);
                    let outcome = transfer::fetch(&agents, source, opts, &ui, index);
                    gate.release(&host);
                    finish(&ui, index, &outcome);
                    results.lock().expect("下载结果锁")[index] = Some(outcome);
                }
            });
        }
    });

    ui.clear();

    let results = results.into_inner().expect("下载结果锁");
    Ok(results
        .into_iter()
        .map(|outcome| outcome.unwrap_or_else(|| Outcome::Failed("没有执行".to_owned())))
        .collect())
}

/// 跟随重定向后的最终地址。按候选地址顺序探测（和安装期同一套镜像逻辑），
/// 发 `Range: bytes=0-0` 只读 1 字节，接受 200 / 206 / 416；全部不可达才报错。
pub fn final_url(url: &str, opts: &Options) -> Result<String> {
    let agents = transfer::agents(opts)?;
    for (position, candidate) in candidates(url, &opts.github_proxy).into_iter().enumerate() {
        let direct = position == 0 && is_github_release(url);
        let agent = if direct {
            &agents.direct
        } else {
            &agents.normal
        };
        if let Ok(probe) = transfer::probe(agent, &candidate) {
            return Ok(probe.url);
        }
    }
    bail!("探测不到可达地址: {url}")
}

/// 拿一个地址的文本内容，跟重定向，返回 (最终地址, 正文)。
/// GitHub API 在 GITHUB_TOKEN 环境变量存在时带 Bearer 鉴权（提升限流额度）；
/// token 只发给 api.github.com，别的站点和源码下载都不带。
pub fn fetch_text(url: &str, opts: &Options) -> Result<(String, String)> {
    let agents = transfer::agents(opts)?;
    let agent = if is_github_release(url) {
        &agents.direct
    } else {
        &agents.normal
    };
    let token = if transfer::host_of(url) == "api.github.com" {
        std::env::var("GITHUB_TOKEN").ok()
    } else {
        None
    };
    let auth = token.map(|token| format!("Bearer {token}"));
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if let Some(value) = &auth {
        headers.push(("Authorization", value.as_str()));
    }
    let (final_url, resp) = transfer::request_with(agent, Method::GET, url, None, &headers)?;
    let status = resp.status().as_u16();
    if status >= 400 {
        bail!("HTTP {status}")
    }
    let body = resp.into_body().read_to_string()?;
    Ok((final_url, body))
}

/// 同一站点的并发许可。GitHub 主站和镜像共用一个桶。
struct HostGate {
    limit: usize,
    state: Mutex<HashMap<String, usize>>,
    cv: Condvar,
}

impl HostGate {
    fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            state: Mutex::new(HashMap::new()),
            cv: Condvar::new(),
        }
    }

    fn acquire(&self, host: &str) {
        let mut state = self.state.lock().expect("站点并发锁");
        while state.get(host).copied().unwrap_or(0) >= self.limit {
            state = self.cv.wait(state).expect("站点并发锁");
        }
        *state.entry(host.to_owned()).or_default() += 1;
    }

    fn release(&self, host: &str) {
        let mut state = self.state.lock().expect("站点并发锁");
        if let Some(count) = state.get_mut(host) {
            *count = count.saturating_sub(1);
        }
        self.cv.notify_all();
    }
}

#[cfg(test)]
mod tests;
