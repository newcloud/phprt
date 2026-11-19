//! 版本区间比较。
//!
//! 声明里写几段就只比几段：`min = "14"` 接受 14.0.1，`max = "13"` 接受 13.9.9、
//! 拒绝 14.0。`tool(min/max)` 和 `toolchain_range(min/max)` 共用这一份实现。

use std::cmp::Ordering;

/// 用 `bound` 写出的段数比较 `value`；`value` 缺的段按 0 补。
pub fn compare_to_bound(value: &str, bound: &str) -> Ordering {
    let mut parts = value.split('.');
    for part in bound.split('.') {
        let right: u64 = part.parse().unwrap_or(0);
        let left: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        match left.cmp(&right) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// 版本是否落在区间里；空上界 / 下界表示不限。
pub fn in_range(value: &str, min: &str, max: &str) -> bool {
    (min.is_empty() || compare_to_bound(value, min) != Ordering::Less)
        && (max.is_empty() || compare_to_bound(value, max) != Ordering::Greater)
}

/// 下界是否落在一个上界里（下界 <= 上界）；两条都按各自写出的段数比较。
pub fn lower_within_upper(lower: &str, upper: &str) -> bool {
    compare_to_bound(lower, upper) != Ordering::Greater
}

/// 两个闭区间是否重叠；空 min / max 表示无界。
pub fn ranges_overlap(left: (&str, &str), right: (&str, &str)) -> bool {
    let (left_min, left_max) = left;
    let (right_min, right_max) = right;
    let left_ok = right_max.is_empty() || lower_within_upper(left_min, right_max);
    let right_ok = left_max.is_empty() || lower_within_upper(right_min, left_max);
    left_ok && right_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_only_compare_the_written_segments() {
        assert!(in_range("15.3.1", "14", ""));
        assert!(in_range("14", "14", ""));
        assert!(!in_range("13.9.9", "14", ""));
        assert!(in_range("13.9.9", "", "13"));
        assert!(in_range("13", "", "13"));
        assert!(!in_range("14.0", "", "13"));
        assert!(in_range("13.5.2", "", "13.5"));
        assert!(!in_range("13.6", "", "13.5"));
    }

    #[test]
    fn open_bounds_accept_everything() {
        assert!(in_range("0.1", "", ""));
        assert!(in_range("99", "", ""));
    }

    #[test]
    fn overlap_respects_prefix_bounds() {
        assert!(!ranges_overlap(("4.8", "13"), ("14", "")));
        assert!(ranges_overlap(("4.8", "13"), ("13.5", "")));
        assert!(ranges_overlap(("4.8", ""), ("16", "")));
        assert!(ranges_overlap(("4.8", "13"), ("", "4.9")));
    }
}

/// 完整版本比较：数字段逐段比较，数字相同时再比后缀（如 `1.1.1w` > `1.1.1u`）；
/// 缺的段按 0。用于 catalog 在一条线的多个补丁里取最大版本。
pub fn compare(left: &str, right: &str) -> std::cmp::Ordering {
    let mut left = left.split('.');
    let mut right = right.split('.');
    loop {
        let l = left.next();
        let r = right.next();
        if l.is_none() && r.is_none() {
            return std::cmp::Ordering::Equal;
        }
        let (left_num, left_rest) = split_segment(l.unwrap_or("0"));
        let (right_num, right_rest) = split_segment(r.unwrap_or("0"));
        match left_num.cmp(&right_num) {
            std::cmp::Ordering::Equal => match left_rest.cmp(right_rest) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            },
            other => return other,
        }
    }
}

fn split_segment(segment: &str) -> (u64, &str) {
    let digits = segment.chars().take_while(|c| c.is_ascii_digit()).count();
    (segment[..digits].parse().unwrap_or(0), &segment[digits..])
}

#[cfg(test)]
mod compare_tests {
    use super::*;

    #[test]
    fn full_compare_orders_patches() {
        assert_eq!(compare("3.5.9", "3.5.8"), std::cmp::Ordering::Greater);
        assert_eq!(compare("3.5.8", "3.5.9"), std::cmp::Ordering::Less);
        assert_eq!(compare("4.0", "4.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare("1.1.1w", "1.1.1u"), std::cmp::Ordering::Greater);
        assert_eq!(compare("8.4.14", "8.4.13"), std::cmp::Ordering::Greater);
    }
}
