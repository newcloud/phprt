/// GitHub 直连失败后按顺序尝试的镜像。配置文件没写时用这份。
pub const DEFAULT_MIRRORS: [&str; 4] = [
    "https://gh-proxy.com/",
    "https://ghfast.top/",
    "https://ghproxy.net/",
    "https://gh.llkk.cc/",
];

/// 来自 `recipes/phprt.toml` 的 GitHub 镜像设置。
#[derive(Debug, Clone)]
pub struct GitHubProxy {
    pub enabled: bool,
    pub urls: Vec<String>,
}

impl Default for GitHubProxy {
    fn default() -> Self {
        Self {
            enabled: true,
            urls: DEFAULT_MIRRORS.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

/// 下载候选：原始地址优先，GitHub 地址再追加镜像。
pub fn candidates(url: &str, proxy: &GitHubProxy) -> Vec<String> {
    let mut out = vec![url.to_owned()];
    if proxy.enabled && is_github_release(url) {
        for mirror in mirrors(proxy) {
            out.push(join_mirror(&mirror, url));
        }
    }
    out
}

fn mirrors(proxy: &GitHubProxy) -> Vec<String> {
    match std::env::var("PHPRT_GITHUB_MIRRORS") {
        Ok(value) if !value.trim().is_empty() => {
            value.split_whitespace().map(str::to_owned).collect()
        }
        // 空字符串关闭镜像，保持兼容。
        Ok(_) => Vec::new(),
        _ => proxy.urls.clone(),
    }
}

fn join_mirror(mirror: &str, url: &str) -> String {
    if mirror.ends_with('/') {
        format!("{mirror}{url}")
    } else {
        format!("{mirror}/{url}")
    }
}

/// 只有 github.com 上的发布包走镜像；codeload / raw 不做重写。
pub fn is_github_release(url: &str) -> bool {
    url.starts_with("https://github.com/")
}

/// 同一站点的并发桶。GitHub 主站和所有镜像共用一个桶，
/// 避免直连失败后所有任务同时压到同一个镜像上。
pub fn host_bucket(url: &str) -> String {
    let Some(host) = host_of(url) else {
        return "local".to_owned();
    };
    match host.as_str() {
        "github.com"
        | "codeload.github.com"
        | "raw.githubusercontent.com"
        | "objects.githubusercontent.com"
        | "api.github.com"
        | "gh-proxy.com"
        | "ghfast.top"
        | "ghproxy.net"
        | "gh.llkk.cc" => "github".to_owned(),
        _ => host,
    }
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split('/').next().unwrap_or(rest);
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = authority.split(':').next().unwrap_or(authority);
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() { None } else { Some(host) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_urls_get_mirrors_in_order() {
        let list = candidates(
            "https://github.com/curl/curl/releases/download/curl-8_23_0/curl-8.23.0.tar.xz",
            &GitHubProxy::default(),
        );
        assert_eq!(list.len(), 5);
        assert_eq!(
            list[0],
            "https://github.com/curl/curl/releases/download/curl-8_23_0/curl-8.23.0.tar.xz"
        );
        assert_eq!(
            list[1],
            "https://gh-proxy.com/https://github.com/curl/curl/releases/download/curl-8_23_0/curl-8.23.0.tar.xz"
        );
    }

    #[test]
    fn other_hosts_have_no_mirrors() {
        let list = candidates(
            "https://www.openssl.org/source/openssl-3.5.9.tar.gz",
            &GitHubProxy::default(),
        );
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn disabled_proxy_keeps_only_the_original_url() {
        let proxy = GitHubProxy {
            enabled: false,
            urls: vec!["https://gh-proxy.com/".to_owned()],
        };
        let list = candidates(
            "https://github.com/curl/curl/releases/download/curl-8_23_0/curl-8.23.0.tar.xz",
            &proxy,
        );
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn github_mirrors_and_main_share_a_bucket() {
        let a = host_bucket("https://github.com/x/y/archive/refs/tags/v1.tar.gz");
        let b =
            host_bucket("https://ghproxy.net/https://github.com/x/y/archive/refs/tags/v1.tar.gz");
        assert_eq!(a, "github");
        assert_eq!(b, "github");
        assert_eq!(
            host_bucket("https://www.php.net/distributions/php-8.5.12.tar.xz"),
            "www.php.net"
        );
    }
}
