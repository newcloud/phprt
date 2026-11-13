use std::io::IsTerminal;

use clap::ValueEnum;
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    /// 终端里用进度框，其他情况逐行输出
    Auto,
    /// 始终逐行输出
    Plain,
    /// 强制进度框
    Progress,
}

/// 一组并行的进度行：下载和解压共用。
pub struct Tracker {
    plain: bool,
    bars: Vec<ProgressBar>,
    tags: Vec<String>,
    mp: Option<MultiProgress>,
    total: usize,
}

impl Tracker {
    pub fn new(tags: Vec<String>, mode: Mode) -> Self {
        let plain = match mode {
            Mode::Plain => true,
            Mode::Progress => {
                if std::io::stderr().is_terminal() {
                    false
                } else {
                    eprintln!("注意: 输出不是终端，改用逐行日志");
                    true
                }
            }
            Mode::Auto => {
                let term_ok = std::env::var("TERM").map(|v| v != "dumb").unwrap_or(false);
                !(std::io::stderr().is_terminal()
                    && term_ok
                    && std::env::var_os("NO_COLOR").is_none())
            }
        };

        let total = tags.len();
        if plain {
            return Self {
                plain,
                bars: Vec::new(),
                tags,
                mp: None,
                total,
            };
        }

        let mp = MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(5));
        let bars = tags
            .iter()
            .map(|tag| {
                let bar = mp.add(ProgressBar::new(0));
                bar.set_style(spinner_style());
                bar.set_message(pad(tag));
                bar
            })
            .collect();
        Self {
            plain,
            bars,
            tags,
            mp: Some(mp),
            total,
        }
    }

    pub fn is_plain(&self) -> bool {
        self.plain
    }

    /// 逐个开始：plain 打印 `[i/n] tag 动词 细节`，进度框把动词放进消息。
    pub fn start(&self, index: usize, verb: &str, detail: Option<&str>) {
        let Some(tag) = self.tags.get(index) else {
            return;
        };
        if self.plain {
            match detail {
                Some(detail) => eprintln!("[{}/{}] {tag} {verb} {detail}", index + 1, self.total),
                None => eprintln!("[{}/{}] {tag} {verb}", index + 1, self.total),
            }
            return;
        }
        if let Some(bar) = self.bars.get(index) {
            bar.set_message(pad(&format!("{tag} {verb}")));
        }
    }

    pub fn set_total(&self, index: usize, total: Option<u64>) {
        if self.plain {
            return;
        }
        let Some(bar) = self.bars.get(index) else {
            return;
        };
        match total {
            Some(total) if total > 0 => {
                bar.set_style(bar_style());
                bar.set_length(total);
            }
            _ => bar.set_style(spinner_style()),
        }
    }

    pub fn set_position(&self, index: usize, position: u64) {
        if let Some(bar) = self.bars.get(index) {
            bar.set_position(position);
        }
    }

    pub fn advance(&self, index: usize, delta: u64) {
        if let Some(bar) = self.bars.get(index) {
            bar.inc(delta);
        }
    }

    pub fn set_message(&self, index: usize, message: &str) {
        if let Some(bar) = self.bars.get(index) {
            bar.set_message(pad(message));
        }
    }

    /// 完成一行：`✓ 完成（13.9 MB）` / `✗ 失败: ...`。
    pub fn finish(&self, index: usize, mark: &str, text: &str) {
        let Some(tag) = self.tags.get(index) else {
            return;
        };
        if self.plain {
            eprintln!("{tag} {mark} {text}");
            return;
        }
        if let Some(bar) = self.bars.get(index) {
            bar.finish_and_clear();
        }
        if let Some(mp) = &self.mp {
            let _ = mp.println(format!("  {} {mark} {text}", pad(tag)));
        }
    }

    /// 不属于任何一行的普通日志；进度框打开时从上方打印，不破坏进度区。
    pub fn line(&self, text: &str) {
        match &self.mp {
            Some(mp) if !self.plain => {
                let _ = mp.println(text);
            }
            _ => eprintln!("{text}"),
        }
    }

    pub fn clear(&self) {
        if let Some(mp) = &self.mp {
            let _ = mp.clear();
        }
    }
}

/// 组件标签：`[php] 8.5.12` / `[deps] openssl 3.5.9`。
/// PHP 的名字和版本相同，只显示一次。
pub fn tag(label: &str, name: &str, version: &str) -> String {
    if name == version {
        format!("[{label}] {version}")
    } else {
        format!("[{label}] {name} {version}")
    }
}

/// 标签固定宽度，避免进度条左右跳动。标签只含 ASCII。
fn pad(tag: &str) -> String {
    format!("{tag:<32}")
}

pub fn human(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const KB: f64 = 1024.0;
    let n = bytes as f64;
    if n >= MB {
        format!("{:.1} MB", n / MB)
    } else if n >= KB {
        format!("{:.0} KB", n / KB)
    } else {
        format!("{bytes} B")
    }
}

const BAR_TEMPLATE: &str =
    "  {msg} [{bar:28}] {percent:>3}% {bytes:>10}/{total_bytes:<10} {bytes_per_sec}";
const SPINNER_TEMPLATE: &str = "  {msg} {spinner} {bytes}";

fn bar_style() -> ProgressStyle {
    ProgressStyle::with_template(BAR_TEMPLATE)
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("█░")
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template(SPINNER_TEMPLATE)
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_templates_are_valid() {
        assert!(ProgressStyle::with_template(BAR_TEMPLATE).is_ok());
        assert!(ProgressStyle::with_template(SPINNER_TEMPLATE).is_ok());
    }

    #[test]
    fn tags_collapse_php_duplicate_name() {
        assert_eq!(tag("php", "8.5.12", "8.5.12"), "[php] 8.5.12");
        assert_eq!(tag("deps", "openssl", "3.5.9"), "[deps] openssl 3.5.9");
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human(512), "512 B");
        assert_eq!(human(2048), "2 KB");
        assert_eq!(human(3 * 1024 * 1024), "3.0 MB");
    }
}
