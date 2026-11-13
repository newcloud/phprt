use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use ureq::http::{Method, Response};
use ureq::{Agent, Body, Proxy};

use super::candidate;
use super::store::{self, PartMeta};
use phprt_source::Source;
use phprt_ui::Tracker;

use super::{Options, Outcome};

/// 小于这个长度不做分片，省下的时间不如多开一个文件。
const MIN_SPLIT: u64 = 8 * 1024 * 1024;
/// 单个分片的最小长度。
const MIN_CHUNK: u64 = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_REDIRECTS: usize = 6;

/// 服务端在探测时声称支持 Range，但实际返回 200。
#[derive(Debug)]
struct RangeUnsupported;

impl std::fmt::Display for RangeUnsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("服务端不支持 Range")
    }
}

impl std::error::Error for RangeUnsupported {}

/// 普通请求用长超时；GitHub 直连单独给一个短超时，失败后尽快换镜像。
pub struct Agents {
    pub normal: Agent,
    pub direct: Agent,
}

pub fn agents(opts: &Options) -> Result<Agents> {
    let direct = direct_timeout();
    Ok(Agents {
        normal: build_agent(opts, opts.timeout)?,
        direct: build_agent(opts, opts.timeout.min(direct))?,
    })
}

fn build_agent(opts: &Options, timeout: u64) -> Result<Agent> {
    let proxy = match &opts.proxy {
        Some(value) => Some(Proxy::new(value).with_context(|| format!("代理地址不合法: {value}"))?),
        None => Proxy::try_from_env(),
    };
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_per_read(Some(READ_TIMEOUT))
        .max_redirects(0)
        .http_status_as_error(false)
        .proxy(proxy)
        .user_agent(concat!("phprt/", env!("CARGO_PKG_VERSION")))
        .build();
    Ok(config.into())
}

fn direct_timeout() -> u64 {
    std::env::var("PHPRT_GITHUB_DIRECT_TIMEOUT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30)
}

/// 缓存命中、下载、校验、失败分类都在这里完成。
pub fn fetch(
    agents: &Agents,
    item: &Source,
    opts: &Options,
    ui: &Tracker,
    index: usize,
) -> Outcome {
    match fetch_inner(agents, item, opts, ui, index) {
        Ok(Fetched::Done(size)) => Outcome::Done(size),
        Ok(Fetched::Skipped(size)) => Outcome::Skipped(size),
        Err(err) => Outcome::Failed(format!("{err:#}")),
    }
}

enum Fetched {
    Done(u64),
    Skipped(u64),
}

fn fetch_inner(
    agents: &Agents,
    item: &Source,
    opts: &Options,
    ui: &Tracker,
    index: usize,
) -> Result<Fetched> {
    let direct = candidate::is_github_release(&item.url);
    let cache_agent = if direct {
        &agents.direct
    } else {
        &agents.normal
    };
    if item.archive.exists() && cache_ok(cache_agent, item, opts).unwrap_or(false) {
        return Ok(Fetched::Skipped(store::size(&item.archive)));
    }
    if opts.offline {
        bail!("离线模式，缓存里没有这个文件");
    }

    let mut last: Option<anyhow::Error> = None;
    let is_github = direct;
    for (position, candidate) in candidate::candidates(&item.url, &opts.github_proxy)
        .into_iter()
        .enumerate()
    {
        // GitHub 直连只给短超时；失败或太慢时尽快换镜像。
        let agent = if position == 0 && is_github {
            &agents.direct
        } else {
            &agents.normal
        };
        // GitHub 直连只试一次，尽快让位给镜像。
        let attempts = if is_github && position == 0 { 1 } else { 2 };
        for attempt in 0..attempts {
            match transfer_once(agent, item, &candidate, opts, ui, index) {
                Ok(_) => match verify_and_finish(item) {
                    Ok(()) => return Ok(Fetched::Done(store::size(&item.archive))),
                    Err(err) => {
                        store::cleanup(&item.archive);
                        last = Some(err);
                        if attempt == 0 {
                            continue;
                        }
                    }
                },
                Err(err) => last = Some(err),
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("没有可用的下载地址")))
}

fn cache_ok(agent: &Agent, item: &Source, opts: &Options) -> Result<bool> {
    if !item.sha256.is_empty() {
        let actual = store::sha256_file(&item.archive)?;
        if actual.eq_ignore_ascii_case(&item.sha256) {
            return Ok(true);
        }
        // 校验不过的文件不能留着，否则以后每次都会走到这里。
        let _ = fs::remove_file(&item.archive);
        return Ok(false);
    }

    if opts.offline {
        eprintln!(
            "注意: {} 没有 sha256，离线模式按已存在处理",
            source_label(item)
        );
        return Ok(true);
    }

    let candidates = candidate::candidates(&item.url, &opts.github_proxy);
    let probe = probe(agent, &candidates[0])?;
    match probe.total {
        Some(total) if total == store::size(&item.archive) => Ok(true),
        Some(_) => {
            let _ = fs::remove_file(&item.archive);
            Ok(false)
        }
        None => {
            eprintln!(
                "注意: {} 没有 sha256，服务端也未提供大小，按已存在处理",
                source_label(item)
            );
            Ok(true)
        }
    }
}

fn verify_and_finish(item: &Source) -> Result<()> {
    let part = store::part_path(&item.archive);
    if !part.exists() {
        bail!("下载结束但没有临时文件");
    }
    if !item.sha256.is_empty() {
        let actual = store::sha256_file(&part)?;
        if !actual.eq_ignore_ascii_case(&item.sha256) {
            bail!("SHA256 不匹配（期望 {}，实际 {actual}）", item.sha256);
        }
    }
    fs::rename(&part, &item.archive)
        .with_context(|| format!("无法写入 {}", item.archive.display()))?;
    let _ = fs::remove_file(store::meta_path(&item.archive));
    Ok(())
}

fn transfer_once(
    agent: &Agent,
    item: &Source,
    candidate: &str,
    opts: &Options,
    ui: &Tracker,
    index: usize,
) -> Result<u64> {
    let probe = probe(agent, candidate)?;
    ui.set_total(index, probe.total);

    if probe.ranges
        && let Some(total) = probe.total
        && total >= MIN_SPLIT
        && opts.connections > 1
    {
        match chunked(agent, item, &probe, opts, ui, index) {
            Ok(size) => return Ok(size),
            Err(err) if err.downcast_ref::<RangeUnsupported>().is_some() => {
                store::cleanup(&item.archive);
            }
            Err(err) => return Err(err),
        }
    }
    single(agent, item, &probe, ui, index)
}

struct Probe {
    /// 用户给出的候选地址（不含跳转后的签名参数），用于续传比对。
    origin: String,
    /// 跳转后的最终地址，实际请求用它。
    url: String,
    total: Option<u64>,
    ranges: bool,
    etag: String,
    last_modified: String,
}

/// 用 `Range: bytes=0-0` 探测总长和分片能力；只读 1 字节。
fn probe(agent: &Agent, url: &str) -> Result<Probe> {
    let (final_url, mut resp) = request(agent, Method::GET, url, Some("bytes=0-0"))?;
    let status = resp.status().as_u16();
    let etag = header(&resp, "etag");
    let last_modified = header(&resp, "last-modified");

    let (total, ranges) = match status {
        206 => (content_range_total(&resp), true),
        200 => (content_length(&resp), false),
        416 => (content_range_total(&resp), true),
        code if code >= 400 => bail!("HTTP {code}"),
        _ => (content_length(&resp), false),
    };
    // 206 的响应体只有 1 字节，读掉；200 说明服务端忽略了 Range，
    // 不要把整个文件读进内存，直接丢弃连接。
    if status == 206 {
        let _ = resp.body_mut().read_to_vec();
    }

    Ok(Probe {
        origin: url.to_owned(),
        url: final_url,
        total,
        ranges,
        etag,
        last_modified,
    })
}

fn chunked(
    agent: &Agent,
    item: &Source,
    probe: &Probe,
    opts: &Options,
    ui: &Tracker,
    index: usize,
) -> Result<u64> {
    let total = probe.total.unwrap_or(0);
    let want = total.div_ceil(MIN_CHUNK) as usize;
    let count = want.min(opts.connections).max(1);
    if count == 1 {
        return single(agent, item, probe, ui, index);
    }
    let bounds = split(total, count);
    let meta = PartMeta {
        url: probe.origin.clone(),
        etag: probe.etag.clone(),
        last_modified: probe.last_modified.clone(),
        total,
        chunks: bounds.len(),
    };
    let meta_path = store::meta_path(&item.archive);
    if store::read_meta(&meta_path).as_ref() != Some(&meta) {
        store::cleanup(&item.archive);
        store::write_meta(&meta_path, &meta)?;
    }

    let mut have = 0;
    for (chunk, (start, end)) in bounds.iter().enumerate() {
        let expected = end - start + 1;
        let path = store::chunk_path(&item.archive, chunk);
        let got = store::size(&path);
        if got > expected {
            let _ = fs::remove_file(&path);
        } else {
            have += got;
        }
    }
    ui.set_total(index, Some(total));
    ui.set_position(index, have);

    let failure: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for (chunk, (start, end)) in bounds.iter().enumerate() {
            let failure = &failure;
            scope.spawn(move || {
                if failure.lock().expect("下载状态锁").is_some() {
                    return;
                }
                if let Err(err) =
                    chunk_worker(agent, &probe.url, item, *start, *end, chunk, ui, index)
                {
                    let mut slot = failure.lock().expect("下载状态锁");
                    if slot.is_none() {
                        *slot = Some(err);
                    }
                }
            });
        }
    });
    if let Some(err) = failure.into_inner().expect("下载状态锁") {
        store::cleanup(&item.archive);
        return Err(err);
    }

    ui.set_message(index, "校验中");
    let part = store::part_path(&item.archive);
    let mut out = File::create(&part).with_context(|| format!("写不到 {}", part.display()))?;
    let mut buf = [0u8; 64 * 1024];
    for chunk in 0..bounds.len() {
        let path = store::chunk_path(&item.archive, chunk);
        let mut file = File::open(&path).with_context(|| format!("读不到 {}", path.display()))?;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
        }
        let _ = fs::remove_file(&path);
    }
    out.flush()?;
    drop(out);
    let _ = fs::remove_file(&meta_path);

    if store::size(&part) != total {
        bail!(
            "分片拼装后长度不对（期望 {total}，实际 {}）",
            store::size(&part)
        );
    }
    Ok(total)
}

/// 连续 60 秒平均速度低于 1 KB/s 就放弃，避免慢速连接把整个任务拖住。
struct SpeedGuard {
    window: Instant,
    window_bytes: u64,
    started: Instant,
}

impl SpeedGuard {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            window: now,
            window_bytes: 0,
            started: now,
        }
    }

    fn add(&mut self, bytes: u64) -> Result<()> {
        self.window_bytes += bytes;
        let elapsed = self.window.elapsed();
        if elapsed >= Duration::from_secs(10) {
            let rate = self.window_bytes as f64 / elapsed.as_secs_f64();
            if self.started.elapsed() >= Duration::from_secs(60) && rate < 1024.0 {
                bail!("下载速度持续低于 1 KB/s");
            }
            self.window = Instant::now();
            self.window_bytes = 0;
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn chunk_worker(
    agent: &Agent,
    url: &str,
    item: &Source,
    start: u64,
    end: u64,
    chunk: usize,
    ui: &Tracker,
    index: usize,
) -> Result<()> {
    let expected = end - start + 1;
    let path = store::chunk_path(&item.archive, chunk);
    let mut have = store::size(&path);
    if have > expected {
        let _ = fs::remove_file(&path);
        have = 0;
    }
    if have == expected {
        return Ok(());
    }

    let range = format!("bytes={}-{}", start + have, end);
    let (_url, mut resp) = request(agent, Method::GET, url, Some(&range))?;
    let status = resp.status().as_u16();
    if status == 200 {
        return Err(anyhow::Error::new(RangeUnsupported));
    }
    if status != 206 {
        bail!("HTTP {status}");
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("写不到 {}", path.display()))?;
    let mut reader = resp.body_mut().as_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut guard = SpeedGuard::new();
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        ui.advance(index, n as u64);
        guard.add(n as u64)?;
    }
    let got = store::size(&path);
    if got != expected {
        bail!("分片 {chunk} 长度不对（期望 {expected}，实际 {got}）");
    }
    Ok(())
}

fn single(agent: &Agent, item: &Source, probe: &Probe, ui: &Tracker, index: usize) -> Result<u64> {
    let part = store::part_path(&item.archive);
    let mut have = store::size(&part);
    if let Some(total) = probe.total {
        if have > total {
            let _ = fs::remove_file(&part);
            have = 0;
        } else if have == total && total > 0 {
            ui.set_total(index, Some(total));
            ui.set_position(index, have);
            return Ok(total);
        }
    }

    let range = (have > 0).then(|| format!("bytes={have}-"));
    let (_url, mut resp) = request(agent, Method::GET, &probe.url, range.as_deref())?;
    let status = resp.status().as_u16();
    if status == 416 && probe.total == Some(have) {
        return Ok(have);
    }
    if status != 200 && status != 206 {
        bail!("HTTP {status}");
    }
    if status == 200 {
        have = 0;
    }
    let total = if status == 206 {
        probe.total.or_else(|| content_range_total(&resp))
    } else {
        content_length(&resp).or(probe.total)
    };
    ui.set_total(index, total);
    ui.set_position(index, have);

    let mut file = if status == 206 {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part)
            .with_context(|| format!("写不到 {}", part.display()))?
    } else {
        File::create(&part).with_context(|| format!("写不到 {}", part.display()))?
    };
    let mut reader = resp.body_mut().as_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut guard = SpeedGuard::new();
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        ui.advance(index, n as u64);
        guard.add(n as u64)?;
    }
    let size = store::size(&part);
    if let Some(total) = probe.total
        && size != total
    {
        bail!("下载不完整（期望 {total}，实际 {size}）");
    }
    Ok(size)
}

/// 发起请求并手动跟随重定向，这样 Range 头不会被中间跳转丢掉。
fn request(
    agent: &Agent,
    method: Method,
    url: &str,
    range: Option<&str>,
) -> Result<(String, Response<Body>)> {
    let mut current = url.to_owned();
    for _ in 0..MAX_REDIRECTS {
        let mut req = if method == Method::GET {
            agent.get(&current)
        } else {
            agent.head(&current)
        };
        if let Some(range) = range {
            req = req.header("Range", range);
        }
        let resp = req.call().with_context(|| format!("请求失败: {current}"))?;
        if !resp.status().is_redirection() {
            return Ok((current, resp));
        }
        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let Some(location) = location else {
            return Ok((current, resp));
        };
        current = join_url(&current, &location)?;
    }
    bail!("重定向次数过多: {url}")
}

fn join_url(base: &str, location: &str) -> Result<String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(location.to_owned());
    }
    let (scheme, rest) = base
        .split_once("://")
        .ok_or_else(|| anyhow!("URL 不合法: {base}"))?;
    if let Some(rest) = location.strip_prefix("//") {
        return Ok(format!("{scheme}://{rest}"));
    }
    if location.starts_with('/') {
        let authority = rest.split('/').next().unwrap_or(rest);
        return Ok(format!("{scheme}://{authority}{location}"));
    }
    let dir = match base.rfind('/') {
        Some(i) => &base[..i + 1],
        None => base,
    };
    Ok(format!("{dir}{location}"))
}

fn header(resp: &Response<Body>, name: &str) -> String {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

fn content_length(resp: &Response<Body>) -> Option<u64> {
    header(resp, "content-length").parse().ok()
}

/// `Content-Range: bytes 0-0/12345` -> 12345
fn content_range_total(resp: &Response<Body>) -> Option<u64> {
    let value = header(resp, "content-range");
    let (_, total) = value.rsplit_once('/')?;
    total.trim().parse().ok()
}

fn split(total: u64, count: usize) -> Vec<(u64, u64)> {
    let count = count.max(1) as u64;
    (0..count)
        .map(|i| {
            let start = total * i / count;
            let end = total * (i + 1) / count - 1;
            (start, end)
        })
        .filter(|(start, end)| start <= end)
        .collect()
}

fn source_label(item: &Source) -> String {
    phprt_ui::tag(&item.label, &item.name, &item.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_covers_the_whole_file() {
        let parts = split(10 * MIN_CHUNK + 7, 4);
        assert_eq!(parts.first().unwrap().0, 0);
        assert_eq!(parts.last().unwrap().1, 10 * MIN_CHUNK + 6);
        for pair in parts.windows(2) {
            assert_eq!(pair[0].1 + 1, pair[1].0);
        }
    }

    #[test]
    fn join_url_handles_absolute_and_relative_locations() {
        assert_eq!(
            join_url("https://a.example/x/y", "https://b.example/z").unwrap(),
            "https://b.example/z"
        );
        assert_eq!(
            join_url("https://a.example/x/y", "/z").unwrap(),
            "https://a.example/z"
        );
        assert_eq!(
            join_url("https://a.example/x/y", "z").unwrap(),
            "https://a.example/x/z"
        );
    }
}
