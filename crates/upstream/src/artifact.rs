//! 下载连接的验证和 sha256。

/// 地址里的文件名版本号必须和 version 一致（docs/CATALOG-UPDATE.md 7）。
/// catalog 里存的是脚本提取的稳定地址，不是跟随重定向后的临时地址。
pub fn filename_matches(url: &str, version: &str) -> bool {
    let name = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .unwrap_or(url);
    name.contains(version)
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
}
