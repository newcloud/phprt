mod candidate;
mod store;
mod transfer;

pub use candidate::{GitHubProxy, candidates, host_bucket, is_github_release};
pub use store::{archive_name, sha256_file};

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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

pub fn run(sources: &[Source], cache: &Path, opts: &Options) -> Result<()> {
    let results = run_outcomes(sources, cache, opts)?;
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
pub fn run_outcomes(sources: &[Source], cache: &Path, opts: &Options) -> Result<Vec<Outcome>> {
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    fs::create_dir_all(cache).with_context(|| format!("建不了 {}", cache.display()))?;

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

/// GitHub API 报过限流后的恢复时间（epoch 秒）；0 表示还没遇到。
/// 进程内记住一次，后续组件不再发注定 403 的请求。
static GITHUB_LIMIT_RESET: AtomicU64 = AtomicU64::new(0);

/// GitHub API 是否已经报过限流；返回恢复时间（epoch 秒）。
pub fn github_rate_limit_reset() -> Option<u64> {
    let reset = GITHUB_LIMIT_RESET.load(Ordering::Relaxed);
    (reset > 0).then_some(reset)
}

/// 地址是不是 GitHub REST API。限流按出口 IP 算，和 User-Agent 无关。
pub fn is_github_api(url: &str) -> bool {
    transfer::host_of(url) == "api.github.com"
}

/// 限流说明：多久恢复、怎么提高额度。`reset` 是 epoch 秒，未知时传 None。
pub fn github_rate_limit_note(reset: Option<u64>) -> String {
    let when = match reset {
        Some(reset) => {
            let minutes = reset.saturating_sub(now_epoch()).div_ceil(60);
            match minutes {
                0 => "不到 1 分钟后恢复".to_owned(),
                1 => "约 1 分钟后恢复".to_owned(),
                n => format!("约 {n} 分钟后恢复"),
            }
        }
        None => "额度稍后恢复".to_owned(),
    };
    format!(
        "GitHub API 限流：匿名额度 60 次/小时已用完，{when}；\
         设置 GITHUB_TOKEN 可提升到 5000 次/小时"
    )
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// 本轮 GitHub 主站直连失败过；后面的查询直接走镜像，不再每个组件等一次超时。
static GITHUB_DIRECT_FAILED: AtomicBool = AtomicBool::new(false);

/// 拿一个地址的文本内容，跟重定向，返回 (最终地址, 正文)。
/// GitHub API 在 GITHUB_TOKEN 环境变量存在时带 Bearer 鉴权（提升限流额度）；
/// token 只发给 api.github.com，别的站点和源码下载都不带。
/// GitHub 主站在部分网络不可直连，连接失败后按下载用的同一份镜像列表重试；
/// 原始地址返回状态码错误时不换镜像（同一份内容，换地址结果一样），镜像自己出错才换。
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
    let list = candidates(url, &opts.github_proxy);
    let skip_direct =
        list.len() > 1 && is_github_release(url) && GITHUB_DIRECT_FAILED.load(Ordering::Relaxed);
    let mut last: Option<anyhow::Error> = None;
    for (index, candidate) in list.into_iter().enumerate() {
        if index == 0 && skip_direct {
            continue;
        }
        match fetch_text_once(agent, &candidate, &headers) {
            Ok(pair) => return Ok(pair),
            // 原始地址的状态码错误不再换镜像；镜像自己出错（403 / 404）换下一个。
            Err(Failure::Status(err)) => {
                if index == 0 {
                    return Err(err);
                }
                last = Some(err);
            }
            Err(Failure::Transport(err)) => {
                if index == 0 {
                    GITHUB_DIRECT_FAILED.store(true, Ordering::Relaxed);
                }
                last = Some(err);
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("拿不到内容: {url}")))
}

/// 单次请求的失败类型：状态码错误直接结束，连接错误可以换镜像再试。
enum Failure {
    Status(anyhow::Error),
    Transport(anyhow::Error),
}

fn fetch_text_once(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
) -> std::result::Result<(String, String), Failure> {
    let (final_url, resp) = transfer::request_with(agent, Method::GET, url, None, headers)
        .map_err(Failure::Transport)?;
    let status = resp.status().as_u16();
    if status >= 400 {
        // 报错要带请求地址和上游原文：403 常见的是 GitHub 限流，只报状态码看不出原因。
        let remaining = header_number(&resp, "x-ratelimit-remaining");
        let reset = header_number(&resp, "x-ratelimit-reset");
        let body = resp.into_body().read_to_string().unwrap_or_default();
        return Err(Failure::Status(http_status_error(
            &final_url, status, remaining, reset, &body,
        )));
    }
    let body = resp
        .into_body()
        .read_to_string()
        .map_err(|err| Failure::Transport(err.into()))?;
    Ok((final_url, body))
}

fn header_number(resp: &ureq::http::Response<ureq::Body>, name: &str) -> Option<u64> {
    resp.headers().get(name)?.to_str().ok()?.trim().parse().ok()
}

/// HTTP 状态码错误：`HTTP <码> <地址>: <上游 message>`。
/// GitHub API 的 403 / 429 是匿名额度用完时，追加恢复时间和 GITHUB_TOKEN 提示，
/// 并记下恢复时间，让本轮后续组件直接跳过。
fn http_status_error(
    url: &str,
    status: u16,
    remaining: Option<u64>,
    reset: Option<u64>,
    body: &str,
) -> anyhow::Error {
    let message = json_message(body).unwrap_or_else(|| clip(body));
    let limited = is_github_api(url)
        && matches!(status, 403 | 429)
        && (remaining == Some(0) || message.to_ascii_lowercase().contains("rate limit"));
    let reset = reset.filter(|value| *value > 0);
    if limited && let Some(reset) = reset {
        GITHUB_LIMIT_RESET.store(reset, Ordering::Relaxed);
    }
    let mut text = if message.is_empty() {
        format!("HTTP {status} {url}")
    } else {
        format!("HTTP {status} {url}: {message}")
    };
    if limited {
        text.push_str(&format!("（{}）", github_rate_limit_note(reset)));
    }
    anyhow::anyhow!(text)
}

/// JSON 返回里的 `message`（GitHub API 的错误说明）；不是 JSON 或没有这个字段
/// 时返回 None，交给原始正文。
fn json_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let message = value.get("message")?.as_str()?.trim();
    (!message.is_empty()).then(|| clip(message))
}

/// 压成一行并截断，避免把一整页 HTML 塞进错误信息。
fn clip(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= 300 {
        text
    } else {
        format!("{}…", text.chars().take(300).collect::<String>())
    }
}

/// 同一站点的并发许可。GitHub 主站和镜像共用一个桶。
/// catalog 查询也用它限流（10 个并发、同站 2 个，和下载一致）。
pub struct HostGate {
    limit: usize,
    state: Mutex<HashMap<String, usize>>,
    cv: Condvar,
}

impl HostGate {
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(1),
            state: Mutex::new(HashMap::new()),
            cv: Condvar::new(),
        }
    }

    pub fn acquire(&self, host: &str) {
        let mut state = self.state.lock().expect("站点并发锁");
        while state.get(host).copied().unwrap_or(0) >= self.limit {
            state = self.cv.wait(state).expect("站点并发锁");
        }
        *state.entry(host.to_owned()).or_default() += 1;
    }

    pub fn release(&self, host: &str) {
        let mut state = self.state.lock().expect("站点并发锁");
        if let Some(count) = state.get_mut(host) {
            *count = count.saturating_sub(1);
        }
        self.cv.notify_all();
    }
}

#[cfg(test)]
mod tests;
