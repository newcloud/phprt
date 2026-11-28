//! 版本比较与线归属。
//!
//! 两个目标共用这一套比较逻辑：
//! 1. 构建时（install / add / remove / update）只指定主线版本时，默认取线内最大补丁；
//! 2. catalog 更新时，从脚本提取的候选里判断哪个下载连接是本次要写的版本。
//!
//! 比较器分两类，用 `classify` 的正则区分（见 docs/CATALOG-UPDATE.md 13.3）：
//!
//! - 点分数字，末段可带字母后缀：`3.5.9`、`1.1.1w`、`8.6.0RC3`；预发布后缀
//!   （alpha / beta / rc / dev / pre）排在无后缀正式版之前；
//! - 点分数字 + 连字符数字后缀：`7.1.2-35`、`2.1-20261102`（后缀按数值比）。
//!
//! 新增不规则形式时在这里加比较器，并在 `classify` 登记。

use std::cmp::Ordering;

/// 版本的比较器类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// 点分数字，末段可带字母后缀。
    Dotted,
    /// 点分数字 + 连字符数字后缀。
    DashNumber,
}

/// 连字符后缀是纯数字、且基础部分每段都是数字的版本，归 DashNumber。
fn classify(version: &str) -> Kind {
    let Some((base, suffix)) = version.rsplit_once('-') else {
        return Kind::Dotted;
    };
    let numeric = !base.is_empty()
        && base
            .split('.')
            .all(|seg| !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_digit()))
        && !suffix.is_empty()
        && suffix.bytes().all(|b| b.is_ascii_digit());
    if numeric {
        Kind::DashNumber
    } else {
        Kind::Dotted
    }
}

/// 两个版本的大小；同类走对应比较器，异类退回字符串比较（同一线内形式一致，不会发生）。
pub fn compare(left: &str, right: &str) -> Ordering {
    match (classify(left), classify(right)) {
        (Kind::Dotted, Kind::Dotted) => compare_dotted(left, right),
        (Kind::DashNumber, Kind::DashNumber) => compare_dash(left, right),
        (_, _) => left.cmp(right),
    }
}

const PRERELEASE_MARKERS: [&str; 5] = ["alpha", "beta", "rc", "dev", "pre"];

/// 点分数字比较：逐段比数字，数字相同再比后缀（如 `1.1.1w > 1.1.1u`）；缺的段按 0 补。
fn compare_dotted(left: &str, right: &str) -> Ordering {
    let mut left = left.split('.');
    let mut right = right.split('.');
    loop {
        let l = left.next();
        let r = right.next();
        if l.is_none() && r.is_none() {
            return Ordering::Equal;
        }
        let (left_num, left_rest) = split_segment(l.unwrap_or("0"));
        let (right_num, right_rest) = split_segment(r.unwrap_or("0"));
        match left_num.cmp(&right_num) {
            Ordering::Equal => match compare_suffix(left_rest, right_rest) {
                Ordering::Equal => continue,
                other => return other,
            },
            other => return other,
        }
    }
}

/// 同一数字段的后缀：预发布排在正式版之前，补丁字母仍按字符串比较。
fn compare_suffix(left: &str, right: &str) -> Ordering {
    match (is_prerelease(left), is_prerelease(right)) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => left.cmp(right),
    }
}

fn is_prerelease(suffix: &str) -> bool {
    let marker = suffix
        .trim_start_matches(|c: char| !c.is_ascii_alphabetic())
        .to_ascii_lowercase();
    PRERELEASE_MARKERS
        .iter()
        .any(|name| marker.starts_with(name))
}

/// 连字符数字后缀：基础部分走点分比较，后缀按数值比（`7.1.2-10 > 7.1.2-9`）。
fn compare_dash(left: &str, right: &str) -> Ordering {
    let (left_base, left_suffix) = left.rsplit_once('-').expect("DashNumber 形式一定有连字符");
    let (right_base, right_suffix) = right.rsplit_once('-').expect("DashNumber 形式一定有连字符");
    match compare_dotted(left_base, right_base) {
        Ordering::Equal => {}
        other => return other,
    }
    let left_num: u64 = left_suffix.parse().unwrap_or(0);
    let right_num: u64 = right_suffix.parse().unwrap_or(0);
    left_num.cmp(&right_num)
}

fn split_segment(segment: &str) -> (u64, &str) {
    let digits = segment.chars().take_while(|c| c.is_ascii_digit()).count();
    (segment[..digits].parse().unwrap_or(0), &segment[digits..])
}

/// 预发布过滤：名字含 alpha/beta/rc/dev/pre，或最后一段数字 >= 90
/// （libjpeg-turbo 的 `3.1.90` 表示 3.2.0 beta）。沿用 v2 outdated 的判据。
pub fn is_unstable(version: &str) -> bool {
    let lower = version.to_ascii_lowercase();
    if PRERELEASE_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return true;
    }
    let last = lower.rsplit('.').next().unwrap_or(&lower);
    let digits: String = last.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u64>().is_ok_and(|number| number >= 90)
}

/// 候选版本是否属于 catalog 已有的线：相等、以「线.」开头、以线开头且下一个
/// 字符是字母（openssl 1.1.1w），或以「线-数字」开头（luajit 的
/// `2.1-20261117` 日期快照）。
pub fn belongs_to_line(version: &str, line: &str) -> bool {
    version == line
        || version.starts_with(&format!("{line}."))
        || version.strip_prefix(line).is_some_and(|rest| {
            rest.starts_with(|c: char| c.is_ascii_alphabetic())
                || (rest.starts_with('-') && rest[1..].starts_with(|c: char| c.is_ascii_digit()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_keeps_current_semantics() {
        assert_eq!(compare("3.5.10", "3.5.9"), Ordering::Greater);
        assert_eq!(compare("8.5.13", "8.5.9"), Ordering::Greater);
        assert_eq!(compare("1.1.1w", "1.1.1u"), Ordering::Greater);
        assert_eq!(compare("1.1.1w", "1.1.1"), Ordering::Greater);
        assert_eq!(compare("4.0", "4.0.0"), Ordering::Equal);
        assert_eq!(compare("8.6.0RC3", "8.6.0"), Ordering::Less);
        assert_eq!(compare("8.6.0", "8.6.0RC3"), Ordering::Greater);
        assert_eq!(compare("2.0.0-beta1", "2.0.0"), Ordering::Less);
        assert_eq!(compare("3.5.9", "3.5.9"), Ordering::Equal);
    }

    #[test]
    fn dash_suffix_compares_numerically() {
        assert_eq!(compare("7.1.2-10", "7.1.2-9"), Ordering::Greater);
        assert_eq!(compare("2.1-20261107", "2.1-20261102"), Ordering::Greater);
        assert_eq!(compare("7.1.2-35", "7.1.2-35"), Ordering::Equal);
        assert_eq!(compare("7.1.2-9", "7.1.2-10"), Ordering::Less);
    }

    #[test]
    fn classify_only_takes_numeric_dash_suffixes() {
        assert_eq!(classify("7.1.2-35"), Kind::DashNumber);
        assert_eq!(classify("2.1-20261102"), Kind::DashNumber);
        assert_eq!(classify("3.5.9"), Kind::Dotted);
        assert_eq!(classify("1.1.1w"), Kind::Dotted);
        // 连字符后缀不是纯数字的不归 DashNumber。
        assert_eq!(classify("1.0-alpha1"), Kind::Dotted);
    }

    #[test]
    fn unstable_filters_prereleases_and_ninety_plus() {
        assert!(is_unstable("6.2.6RC1"));
        assert!(is_unstable("3.1.90"));
        assert!(is_unstable("2.0.0-beta1"));
        assert!(is_unstable("8.6.0rc3"));
        assert!(!is_unstable("3.5.9"));
        assert!(!is_unstable("1.1.1w"));
        assert!(!is_unstable("7.1.2-35"));
    }

    #[test]
    fn line_membership() {
        assert!(belongs_to_line("8.5.13", "8.5"));
        assert!(belongs_to_line("1.1.1w", "1.1.1"));
        assert!(belongs_to_line("1.1.1", "1.1.1"));
        assert!(belongs_to_line("2.1-20261117", "2.1"));
        assert!(!belongs_to_line("1.0-alpha1", "1.0"));
        assert!(!belongs_to_line("4.1.0", "4.0"));
        assert!(!belongs_to_line("3.50.1", "3.5"));
        assert!(!belongs_to_line("1.3.2", "1.2"));
    }
}
