//! 下载连接的验证和 sha256。

/// 地址里的文件名版本号必须和 version 一致（docs/CATALOG-UPDATE.md 7）。
/// catalog 里存的是脚本提取的稳定地址，不是跟随重定向后的临时地址。
///
/// 按边界比对，不能退化成子串匹配：`1.3.2` 不能命中 `zlib-1.3.20.tar.gz`，
/// `1.1.1` 也不能命中 `openssl-1.1.1w.tar.gz`。GitHub tag 归档的 `v` 前缀
/// 算边界（`v2.1-20261117.tar.gz` 匹配版本 `2.1-20261117`）。
pub fn filename_matches(url: &str, version: &str) -> bool {
    let name = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .unwrap_or(url);
    name.match_indices(version).any(|(start, matched)| {
        let before = name[..start].chars().next_back();
        let before_v = name[..start].chars().rev().nth(1);
        // GitHub tag 归档的文件名带 v 前缀（v2.1-20261117.tar.gz）；
        // v 前面也得是边界，避免把 rev1.2.3 当成 1.2.3。
        let v_prefix = before == Some('v') && before_v.is_none_or(|c| !c.is_ascii_alphanumeric());
        let after = name[start + matched.len()..].chars().next();
        (v_prefix || before.is_none_or(|c| !c.is_ascii_alphanumeric()))
            && after.is_none_or(|c| !c.is_ascii_alphanumeric())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_contains_the_version() {
        assert!(filename_matches(
            "https://github.com/openssl/openssl/releases/download/openssl-3.5.10/openssl-3.5.10.tar.gz",
            "3.5.10"
        ));
        assert!(filename_matches("https://x/zlib-1.3.2.tar.gz?x=1", "1.3.2"));
        assert!(!filename_matches("https://x/zlib-1.3.2.tar.gz", "1.3.3"));
    }

    #[test]
    fn filename_matching_respects_version_boundaries() {
        // 子串匹配会把 1.3.2 当成 1.3.20 的命中。
        assert!(!filename_matches("https://x/zlib-1.3.20.tar.gz", "1.3.2"));
        assert!(!filename_matches("https://x/zlib-1.3.21.tar.gz", "1.3.2"));
        assert!(filename_matches("https://x/zlib-1.3.20.tar.gz", "1.3.20"));
        // 1.1.1 不能命中 1.1.1w。
        assert!(!filename_matches(
            "https://x/openssl-1.1.1w.tar.gz",
            "1.1.1"
        ));
        assert!(filename_matches(
            "https://x/openssl-1.1.1w.tar.gz",
            "1.1.1w"
        ));
    }

    #[test]
    fn filename_matching_accepts_the_github_tag_v_prefix() {
        assert!(filename_matches(
            "https://github.com/openresty/luajit2/archive/refs/tags/v2.1-20261117.tar.gz",
            "2.1-20261117"
        ));
        assert!(filename_matches("https://x/v1.6.0.tar.gz", "1.6.0"));
        // v 只是版本前缀时才算边界，不能把单词里的 v 当成前缀。
        assert!(!filename_matches("https://x/rev1.2.3.tar.gz", "1.2.3"));
        assert!(!filename_matches("https://x/v12.3.4.tar.gz", "2.3.4"));
    }
}
