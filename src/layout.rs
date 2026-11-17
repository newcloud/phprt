use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::recipe::{Kind, Target};

/// 没配置 `internal_dir` 时使用的目录名。
pub const DEFAULT_INTERNAL_DIR: &str = ".phprt";
/// 没配置 `build_dir` 时使用的目录名。
pub const DEFAULT_BUILD_DIR: &str = "build";

static INTERNAL_DIR: OnceLock<String> = OnceLock::new();
static BUILD_DIR: OnceLock<String> = OnceLock::new();
static BACKUP_DIR: OnceLock<String> = OnceLock::new();

/// 启动时写入三个目录配置；重复设置忽略。`backup` 为 None 时用产品 recipe 的默认位置。
pub fn set_dirs(internal: &str, build: &str, backup: Option<&str>) {
    let _ = INTERNAL_DIR.set(internal.to_owned());
    let _ = BUILD_DIR.set(build.to_owned());
    if let Some(backup) = backup {
        let _ = BACKUP_DIR.set(backup.to_owned());
    }
}

/// 相对路径按项目根解析，绝对路径原样使用。
pub fn resolve(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
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
    resolve(root, internal_dir_name())
}

/// 当前配置的解压目录。
pub fn build_dir_name() -> &'static str {
    BUILD_DIR
        .get()
        .map(String::as_str)
        .unwrap_or(DEFAULT_BUILD_DIR)
}

/// 当前配置的备份目录；None 表示用产品 recipe 的默认位置。
pub fn backup_dir_name() -> Option<&'static str> {
    BACKUP_DIR.get().map(String::as_str)
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
    build_dir_with(build_dir_name(), name, url)
}

fn build_dir_with(base: &str, name: &str, url: &str) -> String {
    let archive = archive_name(url).unwrap_or_else(|| "source".to_owned());
    let stem = archive_stem(&archive);
    format!("{base}/{name}/{stem}")
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
    fn resolve_keeps_absolute_paths() {
        assert_eq!(
            resolve(Path::new("/proj"), "build"),
            PathBuf::from("/proj/build")
        );
        assert_eq!(
            resolve(Path::new("/proj"), "/tmp/build"),
            PathBuf::from("/tmp/build")
        );
    }

    #[test]
    fn build_dir_uses_the_configured_base() {
        assert_eq!(
            build_dir_with("/tmp/b", "php", "https://php.net/php-8.5.12.tar.xz"),
            "/tmp/b/php/php-8.5.12"
        );
        assert_eq!(
            build_dir_with("data/build", "swoole", "https://x/swoole-6.2.5.tgz"),
            "data/build/swoole/swoole-6.2.5"
        );
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
