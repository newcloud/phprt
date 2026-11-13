use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use phprt_ui::{Mode, Tracker};
use sha2::{Digest, Sha256};

/// 一步构建命令。命令参数由上层展开好，这里不再做占位符替换。
#[derive(Debug, Clone)]
pub enum Cmd {
    /// 直接执行程序，参数逐个传，不经过 shell。
    Program { program: String, args: Vec<String> },
    /// 交给 `sh -c`，用于 `make install` 这类 recipe 写成整行的命令。
    Shell(String),
}

impl Cmd {
    fn display(&self) -> String {
        match self {
            Cmd::Program { program, args } => {
                let mut out = shell_quote(program);
                for arg in args {
                    out.push(' ');
                    out.push_str(&shell_quote(arg));
                }
                out
            }
            Cmd::Shell(line) => line.clone(),
        }
    }
}

/// shared 扩展安装完成后写的 ini。
#[derive(Debug, Clone)]
pub struct Ini {
    pub path: PathBuf,
    pub lines: Vec<String>,
}

/// 一个节点的完整构建步骤。
#[derive(Debug, Clone)]
pub struct Step {
    pub tag: String,
    pub cwd: PathBuf,
    /// builtin shared 扩展从 PHP 源码树拷到 cwd。
    pub copy_from: Option<PathBuf>,
    pub commands: Vec<Cmd>,
    pub env: BTreeMap<String, String>,
    /// 相对安装前缀的产物模式，允许一个 `*` 组件（如 `lib/php/extensions/*/curl.so`）。
    pub artifacts: Vec<PathBuf>,
    pub ini: Option<Ini>,
    /// 构建签名内容；变化时先 make distclean。
    pub sig: String,
    pub log: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub ui: Mode,
}

#[derive(Debug, Default)]
pub struct Summary {
    pub built: usize,
    pub skipped: usize,
}

/// 串行执行；失败时返回带日志末尾的错误，调用方不要写状态。
pub fn run(steps: &[Step], opts: &Options) -> Result<Summary> {
    if steps.is_empty() {
        return Ok(Summary::default());
    }

    eprintln!();
    eprintln!("编译并安装（按构建顺序，{} 个）", steps.len());
    let ui = Tracker::new(steps.iter().map(|step| step.tag.clone()).collect(), opts.ui);
    let mut summary = Summary::default();

    for (index, step) in steps.iter().enumerate() {
        match run_one(step, index, &ui) {
            Ok(true) => summary.built += 1,
            Ok(false) => summary.skipped += 1,
            Err(err) => {
                ui.finish(index, "✗", "失败");
                ui.clear();
                return Err(err);
            }
        }
    }

    ui.clear();
    ui.line(&format!(
        "编译完成: {} 个，已构建跳过 {} 个",
        summary.built, summary.skipped
    ));
    Ok(summary)
}

fn run_one(step: &Step, index: usize, ui: &Tracker) -> Result<bool> {
    let sig_path = step.cwd.join(".phprt-sig");
    let sig = sig_hash(&step.sig);
    let sig_ok = fs::read_to_string(&sig_path)
        .map(|old| old.trim() == sig.as_str())
        .unwrap_or(false);
    let all_artifacts = !step.artifacts.is_empty() && step.artifacts.iter().all(|p| glob_exists(p));

    if sig_ok && all_artifacts {
        if let Some(ini) = &step.ini {
            write_ini(ini)?;
        }
        ui.finish(index, "✓", "已构建");
        return Ok(false);
    }

    if !step.cwd.is_dir() {
        match &step.copy_from {
            Some(source) if source.is_dir() => {
                ui.start(index, "拷贝", Some("PHP 源码"));
                fs::create_dir_all(&step.cwd)
                    .with_context(|| format!("建不了 {}", step.cwd.display()))?;
                copy_dir(source, &step.cwd)?;
            }
            Some(source) => bail!("{} 的源码目录不存在: {}", step.tag, source.display()),
            None => bail!("{} 的源码目录不存在: {}", step.tag, step.cwd.display()),
        }
    }

    // PHP 源码里有些 builtin 扩展只有 config0.m4，phpize 只认 config.m4。
    if step.copy_from.is_some() {
        let config = step.cwd.join("config.m4");
        let config0 = step.cwd.join("config0.m4");
        if !config.exists() && config0.is_file() {
            fs::copy(&config0, &config)
                .with_context(|| format!("复制不了 {}", config0.display()))?;
        }
    }

    if let Some(parent) = step.log.parent() {
        fs::create_dir_all(parent).with_context(|| format!("建不了 {}", parent.display()))?;
    }
    let log =
        File::create(&step.log).with_context(|| format!("写不了日志 {}", step.log.display()))?;
    let mut log = BufWriter::new(log);
    writeln!(log, "==> {}", step.tag)?;
    if let Some(ini) = &step.ini {
        writeln!(log, "ini: {}", ini.path.display())?;
    }

    if !sig_ok && step.cwd.join("Makefile").is_file() {
        ui.start(index, "清理", Some("参数变化，make distclean"));
        let _ = clear_make(&step.env, &step.cwd, &mut log);
    }

    for cmd in &step.commands {
        let label = command_label(cmd);
        ui.start(index, label, None);
        writeln!(log, "$ {}", cmd.display())?;
        log.flush()?;
        let status = match cmd {
            Cmd::Program { program, args } => Command::new(program)
                .args(args)
                .current_dir(&step.cwd)
                .envs(&step.env)
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.get_ref().try_clone()?))
                .stderr(Stdio::from(log.get_ref().try_clone()?))
                .status(),
            Cmd::Shell(line) => Command::new("sh")
                .arg("-c")
                .arg(line)
                .current_dir(&step.cwd)
                .envs(&step.env)
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.get_ref().try_clone()?))
                .stderr(Stdio::from(log.get_ref().try_clone()?))
                .status(),
        };
        let status =
            status.with_context(|| format!("跑不起来 {}（{}）", cmd.display(), step.tag))?;
        if !status.success() {
            log.flush()?;
            let tail = tail_of(&step.log, 40).unwrap_or_default();
            bail!(
                "{} 的 {} 失败（退出码 {}）\n日志: {}\n{}",
                step.tag,
                label,
                status.code().unwrap_or(-1),
                step.log.display(),
                tail
            );
        }
    }

    if let Some(ini) = &step.ini {
        write_ini(ini)?;
        writeln!(log, "已写 ini: {}", ini.path.display())?;
    }

    let missing: Vec<String> = step
        .artifacts
        .iter()
        .filter(|p| !glob_exists(p))
        .map(|p| p.display().to_string())
        .collect();
    if !missing.is_empty() {
        bail!(
            "{} 构建结束但缺少产物:\n  {}",
            step.tag,
            missing.join("\n  ")
        );
    }

    fs::write(&sig_path, format!("{sig}\n"))
        .with_context(|| format!("写不了签名 {}", sig_path.display()))?;
    ui.finish(index, "✓", "完成");
    Ok(true)
}

fn clear_make(env: &BTreeMap<String, String>, cwd: &Path, log: &mut BufWriter<File>) -> Result<()> {
    writeln!(log, "$ make distclean")?;
    let out = Command::new("sh")
        .arg("-c")
        .arg("make distclean")
        .current_dir(cwd)
        .envs(env)
        .stdin(Stdio::null())
        .output();
    if let Ok(out) = out {
        let _ = log.write_all(&out.stdout);
        let _ = log.write_all(&out.stderr);
    }
    Ok(())
}

fn command_label(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Program { program, .. } => {
            let name = Path::new(program)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(program);
            match name {
                "phpize" => "phpize",
                "configure" => "configure",
                _ => "生成",
            }
        }
        Cmd::Shell(line) => {
            let name = line.split_whitespace().next().unwrap_or("sh");
            match name {
                "make" => {
                    if line.contains("install") {
                        "install"
                    } else {
                        "make"
                    }
                }
                _ => "命令",
            }
        }
    }
}

fn write_ini(ini: &Ini) -> Result<()> {
    if let Some(parent) = ini.path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("建不了 {}", parent.display()))?;
    }
    let mut text = String::from("; 由 phprt 生成，勿手改\n");
    for line in &ini.lines {
        text.push_str(line);
        text.push('\n');
    }
    fs::write(&ini.path, text).with_context(|| format!("写不了 {}", ini.path.display()))?;
    Ok(())
}

fn sig_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn tail_of(path: &Path, lines: usize) -> Result<String> {
    let text = fs::read_to_string(path)?;
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    Ok(all[start..].join("\n"))
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@+".contains(c))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// 产物模式检查：路径组件里允许一个 `*`。
fn glob_exists(pattern: &Path) -> bool {
    let text = pattern.to_string_lossy();
    let parts: Vec<&str> = text.split('/').collect();
    if text.starts_with('/') {
        walk_glob(Path::new("/"), &parts[1..])
    } else {
        walk_glob(Path::new("."), &parts)
    }
}

fn walk_glob(dir: &Path, parts: &[&str]) -> bool {
    let Some((part, rest)) = parts.split_first() else {
        return dir.exists();
    };
    if part.is_empty() {
        return walk_glob(dir, rest);
    }
    if part.contains('*') {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        return entries.filter_map(|entry| entry.ok()).any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            wildcard(part, &name) && walk_glob(&entry.path(), rest)
        });
    }
    walk_glob(&dir.join(part), rest)
}

fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut pi = 0usize;
    let mut ti = 0usize;
    let mut star: Option<usize> = None;
    let mut match_at = 0usize;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            match_at = ti;
            pi += 1;
        } else if let Some(star_pos) = star {
            pi = star_pos + 1;
            match_at += 1;
            ti = match_at;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn copy_dir(source: &Path, dest: &Path) -> Result<()> {
    for entry in fs::read_dir(source).with_context(|| format!("读不到 {}", source.display()))? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let meta = fs::symlink_metadata(&from)?;
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&from)?;
            std::os::unix::fs::symlink(target, &to)
                .with_context(|| format!("建不了软链 {}", to.display()))?;
        } else if meta.is_dir() {
            fs::create_dir_all(&to).with_context(|| format!("建不了 {}", to.display()))?;
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .with_context(|| format!("复制不了 {} -> {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matches_names() {
        assert!(wildcard("*.so", "curl.so"));
        assert!(wildcard("libssl.so.*", "libssl.so.3"));
        assert!(!wildcard("*.so", "curl.a"));
        assert!(wildcard("*", "anything"));
    }

    #[test]
    fn shell_quote_keeps_simple_tokens() {
        assert_eq!(shell_quote("--prefix=/a/b"), "--prefix=/a/b");
        assert_eq!(shell_quote("a b"), "'a b'");
    }

    #[test]
    fn signature_is_stable() {
        assert_eq!(sig_hash("a\nb"), sig_hash("a\nb"));
        assert_ne!(sig_hash("a\nb"), sig_hash("a\nc"));
    }
}
