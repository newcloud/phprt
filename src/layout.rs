use crate::recipe::{Kind, Target};

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

/// 解压目录，相对于项目根：build/<组件>/<压缩包 stem>。
/// 这就是源码根，phpize / configure 都在这个目录里执行。
pub fn build_dir(name: &str, kind: Kind, url: &str) -> String {
    let archive = archive_name(url).unwrap_or_else(|| "source".to_owned());
    let stem = archive_stem(&archive);
    let dir_name = if kind == Kind::Php { "php" } else { name };
    format!("build/{dir_name}/{stem}")
}

/// 下载清单里各节点的分类名。
pub fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Php => "php",
        Kind::Dep => "deps",
        Kind::Ext => "ext",
    }
}

/// 下载/解压清单只关心 PHP、依赖库和 PECL 扩展。
pub fn downloadable(target: &Target) -> bool {
    match target.kind {
        Kind::Php | Kind::Dep => true,
        Kind::Ext => target.origin == "pecl",
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
            build_dir("php", Kind::Php, "https://php.net/php-8.5.12.tar.xz"),
            "build/php/php-8.5.12"
        );
        assert_eq!(
            build_dir("libcurl", Kind::Dep, "https://curl.se/curl-8.23.0.tar.xz"),
            "build/libcurl/curl-8.23.0"
        );
        assert_eq!(
            build_dir(
                "swoole",
                Kind::Ext,
                "https://pecl.php.net/get/swoole-6.2.5.tgz"
            ),
            "build/swoole/swoole-6.2.5"
        );
    }
}
