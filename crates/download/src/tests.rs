use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::store::{self, PartMeta};
use super::transfer;

use phprt_source::Source;
use phprt_ui::{Mode, Tracker, tag};

use super::{HostGate, Options, Outcome};

// ---------------------------------------------------------------------------
// 最小 HTTP 服务：够用来测 Range、重定向、重试和大小校验。
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Hit {
    path: String,
    range: Option<String>,
}

struct Server {
    base: String,
    stop: Arc<AtomicBool>,
    hits: Arc<Mutex<Vec<Hit>>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn start(payload: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定测试端口");
        let port = listener.local_addr().expect("取测试端口").port();
        listener.set_nonblocking(true).expect("设置非阻塞");
        let stop = Arc::new(AtomicBool::new(false));
        let hits = Arc::new(Mutex::new(Vec::new()));
        let payload = Arc::new(payload);
        let flaky = Arc::new(AtomicBool::new(true));
        let stop_thread = stop.clone();
        let hits_thread = hits.clone();
        let join = std::thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let payload = payload.clone();
                        let hits = hits_thread.clone();
                        let flaky = flaky.clone();
                        std::thread::spawn(move || {
                            let _ = serve(stream, &payload, &hits, &flaky);
                        });
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            base: format!("http://127.0.0.1:{port}"),
            stop,
            hits,
            join: Some(join),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn hits(&self) -> Vec<Hit> {
        self.hits.lock().expect("测试命中记录").clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn serve(
    mut stream: TcpStream,
    payload: &[u8],
    hits: &Mutex<Vec<Hit>>,
    flaky: &AtomicBool,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request = String::new();
    reader.read_line(&mut request)?;
    let mut parts = request.split_whitespace();
    let _method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/").to_string();

    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("range")
        {
            range = Some(value.trim().to_owned());
        }
    }

    hits.lock().expect("测试命中记录").push(Hit {
        path: path.clone(),
        range: range.clone(),
    });

    match path.as_str() {
        "/redirect" => respond(
            &mut stream,
            "302 Found",
            &[
                ("Location", "/file".to_owned()),
                ("Content-Length", "0".to_owned()),
            ],
            b"",
        ),
        "/fail" => respond(
            &mut stream,
            "500 Internal Server Error",
            &[("Content-Length", "4".to_owned())],
            b"boom",
        ),
        "/flaky" => {
            if flaky.swap(false, Ordering::Relaxed) {
                respond(
                    &mut stream,
                    "500 Internal Server Error",
                    &[("Content-Length", "4".to_owned())],
                    b"boom",
                )
            } else {
                serve_file(&mut stream, payload, range.as_deref(), true)
            }
        }
        "/norange" => serve_file(&mut stream, payload, range.as_deref(), false),
        _ => serve_file(&mut stream, payload, range.as_deref(), true),
    }
}

fn serve_file(
    stream: &mut TcpStream,
    payload: &[u8],
    range: Option<&str>,
    support_range: bool,
) -> std::io::Result<()> {
    let total = payload.len() as u64;
    if support_range && let Some((start, end)) = range.and_then(|value| parse_range(value, total)) {
        let body = &payload[start as usize..=end as usize];
        return respond(
            stream,
            "206 Partial Content",
            &[
                ("Content-Length", body.len().to_string()),
                ("Content-Range", format!("bytes {start}-{end}/{total}")),
                ("Accept-Ranges", "bytes".to_owned()),
                ("ETag", "\"v1\"".to_owned()),
            ],
            body,
        );
    }
    respond(
        stream,
        "200 OK",
        &[
            ("Content-Length", payload.len().to_string()),
            ("Accept-Ranges", "bytes".to_owned()),
            ("ETag", "\"v1\"".to_owned()),
        ],
        payload,
    )
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn parse_range(value: &str, total: u64) -> Option<(u64, u64)> {
    let spec = value.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start: u64 = start.parse().ok()?;
    if start >= total {
        return None;
    }
    let end = if end.is_empty() {
        total - 1
    } else {
        end.parse::<u64>().ok()?.min(total - 1)
    };
    Some((start, end))
}

// ---------------------------------------------------------------------------
// 测试辅助
// ---------------------------------------------------------------------------

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(name: &str) -> PathBuf {
    let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("phprt-dl-{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("建测试目录");
    dir
}

fn agents() -> transfer::Agents {
    fn build() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .timeout_connect(Some(Duration::from_secs(5)))
            .max_redirects(0)
            .http_status_as_error(false)
            .proxy(None)
            .build()
            .into()
    }
    transfer::Agents {
        normal: build(),
        direct: build(),
    }
}

fn options() -> Options {
    Options {
        jobs: 4,
        host_jobs: 2,
        connections: 4,
        timeout: 30,
        proxy: None,
        github_proxy: Default::default(),
        offline: false,
        ui: Mode::Plain,
    }
}

fn sha_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::new();
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn payload(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

fn item(url: &str, sha: Option<&str>, dir: &Path, name: &str) -> Source {
    Source {
        label: "deps".to_owned(),
        name: name.to_owned(),
        version: "1.0".to_owned(),
        url: url.to_owned(),
        sha256: sha.map(str::to_owned).unwrap_or_default(),
        archive: dir.join(name),
        unpack: dir.join("unpack"),
        unpack_display: "build/deps/unpack".to_owned(),
    }
}

fn tracker(items: &[Source]) -> Tracker {
    let tags = items
        .iter()
        .map(|item| tag(&item.label, &item.name, &item.version))
        .collect();
    Tracker::new(tags, Mode::Plain)
}

fn run(item: Source) -> (Outcome, PathBuf) {
    let path = item.archive.clone();
    let items = vec![item];
    let ui = tracker(&items);
    let outcome = transfer::fetch(&agents(), &items[0], &options(), &ui, 0);
    (outcome, path)
}

// ---------------------------------------------------------------------------
// 用例
// ---------------------------------------------------------------------------

#[test]
fn downloads_single_stream_and_verifies() {
    let data = payload(200_000);
    let server = Server::start(data.clone());
    let dir = temp_dir("single");
    let sha = sha_hex(&data);
    let (outcome, file) = run(item(&server.url("/file"), Some(&sha), &dir, "a.bin"));

    assert!(matches!(outcome, Outcome::Done(_)), "{outcome:?}");
    assert_eq!(fs::read(&file).unwrap(), data);
    assert!(!store::part_path(&file).exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn large_file_is_split_and_assembled() {
    let data = payload(9 * 1024 * 1024 + 123);
    let server = Server::start(data.clone());
    let dir = temp_dir("range");
    let sha = sha_hex(&data);
    let (outcome, file) = run(item(&server.url("/file"), Some(&sha), &dir, "big.bin"));

    assert!(matches!(outcome, Outcome::Done(_)), "{outcome:?}");
    assert_eq!(fs::read(&file).unwrap(), data);
    assert!(!store::part_path(&file).exists());

    let hits = server.hits();
    assert!(
        hits.iter().any(|hit| {
            hit.range
                .as_deref()
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.split_once('-'))
                .is_some_and(|(_, end)| end != "0")
        }),
        "应该有分片请求: {hits:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn resumes_a_partial_chunk() {
    let data = payload(9 * 1024 * 1024 + 123);
    let server = Server::start(data.clone());
    let dir = temp_dir("resume");
    let sha = sha_hex(&data);
    // 用会跳转的地址：续传元数据要记用户给的候选地址，
    // 而不是跳转后带签名参数的最终地址。
    let url = server.url("/redirect");
    let item = item(&url, Some(&sha), &dir, "resume.bin");

    store::write_meta(
        &store::meta_path(&item.archive),
        &PartMeta {
            url: url.clone(),
            etag: "\"v1\"".to_owned(),
            last_modified: String::new(),
            total: data.len() as u64,
            chunks: 4,
        },
    )
    .unwrap();
    fs::write(store::chunk_path(&item.archive, 0), &data[..500]).unwrap();

    let (outcome, file) = run(item);
    assert!(matches!(outcome, Outcome::Done(_)), "{outcome:?}");
    assert_eq!(fs::read(&file).unwrap(), data);

    let hits = server.hits();
    assert!(
        hits.iter().any(|hit| hit
            .range
            .as_deref()
            .is_some_and(|r| r.starts_with("bytes=500-"))),
        "应该从 500 字节处续传: {hits:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn follows_redirects_with_range() {
    let data = payload(120_000);
    let server = Server::start(data.clone());
    let dir = temp_dir("redirect");
    let sha = sha_hex(&data);
    let (outcome, file) = run(item(&server.url("/redirect"), Some(&sha), &dir, "r.bin"));

    assert!(matches!(outcome, Outcome::Done(_)), "{outcome:?}");
    assert_eq!(fs::read(&file).unwrap(), data);
    let hits = server.hits();
    assert!(
        hits.iter()
            .any(|hit| hit.path == "/file" && hit.range.as_deref() == Some("bytes=0-0")),
        "跳转后应该保留 Range 探测: {hits:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn retries_after_server_error() {
    let data = payload(120_000);
    let server = Server::start(data.clone());
    let dir = temp_dir("flaky");
    let sha = sha_hex(&data);
    let (outcome, file) = run(item(&server.url("/flaky"), Some(&sha), &dir, "f.bin"));

    assert!(matches!(outcome, Outcome::Done(_)), "{outcome:?}");
    assert_eq!(fs::read(&file).unwrap(), data);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn falls_back_when_server_ignores_range() {
    let data = payload(300_000);
    let server = Server::start(data.clone());
    let dir = temp_dir("norange");
    let sha = sha_hex(&data);
    let (outcome, file) = run(item(&server.url("/norange"), Some(&sha), &dir, "n.bin"));

    assert!(matches!(outcome, Outcome::Done(_)), "{outcome:?}");
    assert_eq!(fs::read(&file).unwrap(), data);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn checksum_mismatch_is_reported() {
    let data = payload(120_000);
    let server = Server::start(data.clone());
    let dir = temp_dir("badsha");
    let wrong = sha_hex(b"not the file");
    let (outcome, file) = run(item(&server.url("/file"), Some(&wrong), &dir, "b.bin"));

    match outcome {
        Outcome::Failed(reason) => assert!(reason.contains("SHA256"), "{reason}"),
        other => panic!("应该失败: {other:?}"),
    }
    assert!(!file.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn existing_file_with_checksum_is_skipped_without_network() {
    let data = payload(120_000);
    let server = Server::start(data.clone());
    let dir = temp_dir("skip");
    let sha = sha_hex(&data);
    let item = item(&server.url("/file"), Some(&sha), &dir, "s.bin");
    fs::write(&item.archive, &data).unwrap();

    let (outcome, file) = run(item);
    assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
    assert_eq!(fs::read(&file).unwrap(), data);
    assert!(server.hits().is_empty(), "不应该发起请求");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn existing_file_without_checksum_uses_remote_size() {
    let data = payload(120_000);
    let server = Server::start(data.clone());
    let dir = temp_dir("nosha");
    let item = item(&server.url("/file"), None, &dir, "n.bin");
    fs::write(&item.archive, &data).unwrap();

    let (outcome, _file) = run(item);
    assert!(matches!(outcome, Outcome::Skipped(_)), "{outcome:?}");
    assert_eq!(server.hits().len(), 1, "只应该做一次大小探测");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn offline_mode_reports_missing_files() {
    let dir = temp_dir("offline");
    let mut opts = options();
    opts.offline = true;
    let item = item("http://127.0.0.1:9/file", None, &dir, "missing.bin");
    let items = vec![item];
    let ui = tracker(&items);
    let outcome = transfer::fetch(&agents(), &items[0], &opts, &ui, 0);

    match outcome {
        Outcome::Failed(reason) => assert!(reason.contains("离线模式"), "{reason}"),
        other => panic!("应该失败: {other:?}"),
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn host_gate_limits_concurrency() {
    let gate = HostGate::new(2);
    let active = AtomicUsize::new(0);
    let max = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let gate = &gate;
            let active = &active;
            let max = &max;
            scope.spawn(move || {
                gate.acquire("github");
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
                gate.release("github");
            });
        }
    });
    assert!(max.load(Ordering::SeqCst) <= 2, "并发超过上限");
}
