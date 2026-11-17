use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::recipe::{Kind, Target};

/// 没配置 `internal_dir` 时使用的目录名。
pub const DEFAULT_INTERNAL_DIR: &str = ".phprt";

static INTERNAL_DIR: OnceLock<String> = OnceLock::new();

/// 由 `phprt.toml` 的 `internal_dir` 在启动时设置；重复设置忽略。
pub fn set_internal_dir_name(name: &str) {
    let _ = INTERNAL_DIR.set(name.to_owned());
}

/// 当前配置的工具目录名。
pub fn internal_dir_name() -> &'static str {
    INTERNAL_DIR
        .get()
        .map(String::as_str)
        .unwrap_or(DEFAULT_INTERNAL_DIR)
}

/// 项目内工具数据目录（状态、安装树、缓存、备份）的根。
pub fn internal_dir(root: &Path) -> PathBuf {
    root.join(internal_dir_name())
}

/// URL 最后一段就是缓存文件名；查询串和锚点不属于文件名。
pub fn archive_name(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

/// 去掉压缩后缀，得到解压目录名。
pub fn archive_stem(name: &str) -> String {
    for suffix in [".tar.gz", ".tar.xz", ".tar.bz2", ".tgz", ".txz", ".zip"] {
        if let Some(stem) = name.strip_suffix(suffix) {
            return stem.to_owned();
        }
    }
    name.to_owned()
}

/// 解压目录，相对于项目根：build/<名字>/<压缩包 stem>。
/// 产品用产品名（如 build/php/php-8.5.12），组件和依赖库用各自的名字。
pub fn build_dir(name: &str, url: &str) -> String {
    let archive = archive_name(url).unwrap_or_else(|| "source".to_owned());
    let stem = archive_stem(&archive);
    format!("build/{name}/{stem}")
}

pub fn build_dir_of(target: &Target) -> String {
    build_dir(&target.name, &target.url)
}

/// 下载清单里各节点的分类名。
pub fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Product => "product",
        Kind::Dep => "deps",
        Kind::Component => "component",
    }
}

/// 下载/解压清单只关心产品本体、依赖库和外部组件。
pub fn downloadable(target: &Target) -> bool {
    match target.kind {
        Kind::Product | Kind::Dep => true,
        Kind::Component => target.source == "external",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_name_strips_query() {
        assert_eq!(
            archive_name("https://pecl.php.net/get/swoole-6.2.5.tgz?x=1").unwrap(),
            "swoole-6.2.5.tgz"
        );
        assert_eq!(
            archive_name("https://www.php.net/distributions/php-8.5.12.tar.xz").unwrap(),
            "php-8.5.12.tar.xz"
        );
        assert!(archive_name("https://example.com/").is_none());
    }

    #[test]
    fn archive_stem_handles_common_suffixes() {
        assert_eq!(archive_stem("openssl-3.5.4.tar.gz"), "openssl-3.5.4");
        assert_eq!(archive_stem("php-8.5.12.tar.xz"), "php-8.5.12");
        assert_eq!(archive_stem("curl-8.11.1.tar.bz2"), "curl-8.11.1");
        assert_eq!(archive_stem("curl-8.11.1.zip"), "curl-8.11.1");
        assert_eq!(archive_stem("plain"), "plain");
    }

    #[test]
    fn build_dir_is_per_component_and_patch() {
        assert_eq!(
            build_dir("php", "https://php.net/php-8.5.12.tar.xz"),
            "build/php/php-8.5.12"
        );
        assert_eq!(
            build_dir("libcurl", "https://curl.se/curl-8.23.0.tar.xz"),
            "build/libcurl/curl-8.23.0"
        );
        assert_eq!(
            build_dir("swoole", "https://pecl.php.net/get/swoole-6.2.5.tgz"),
            "build/swoole/swoole-6.2.5"
        );
        assert_eq!(
            build_dir("nginx", "https://nginx.org/download/nginx-1.29.3.tar.gz"),
            "build/nginx/nginx-1.29.3"
        );
    }
}
