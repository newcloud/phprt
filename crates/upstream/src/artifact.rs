//! 下载连接的验证和 sha256。

/// 地址里的文件名版本号必须和 version 一致（docs/CATALOG-UPDATE.md 7）。
/// catalog 里存的是脚本提取的稳定地址，不是跟随重定向后的临时地址。
///
/// 按边界比对，不能退化成子串匹配：`1.3.2` 不能命中 `zlib-1.3.20.tar.gz`，
/// `1.1.1` 也不能命中 `openssl-1.1.1w.tar.gz`。
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
        let after = name[start + matched.len()..].chars().next();
        before.is_none_or(|c| !c.is_ascii_alphanumeric())
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
}
