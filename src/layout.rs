use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Result, anyhow};

use crate::recipe::{Kind, Target};

/// 没配置 `internal_dir` 时使用的目录名。
pub const DEFAULT_INTERNAL_DIR: &str = ".phprt";
/// 没配置 `build_dir` 时使用的目录名。
pub const DEFAULT_BUILD_DIR: &str = "build";

static INTERNAL_DIR: OnceLock<String> = OnceLock::new();
static BUILD_DIR: OnceLock<String> = OnceLock::new();
static BACKUP_DIR: OnceLock<String> = OnceLock::new();
static GLOBAL_PREFIX: OnceLock<PathBuf> = OnceLock::new();
static DOWNLOAD_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_internal_dir_name(name: &str) {
    let _ = INTERNAL_DIR.set(name.to_owned());
}

pub fn set_build_dir_name(name: &str) {
    let _ = BUILD_DIR.set(name.to_owned());
}

pub fn set_backup_dir_name(name: &str) {
    let _ = BACKUP_DIR.set(name.to_owned());
}

pub fn set_download_dir(dir: PathBuf) {
    let _ = DOWNLOAD_DIR.set(dir);
}

/// `--global --prefix`：产品直接装到这个目录。
pub fn set_global_prefix(prefix: PathBuf) {
    let _ = GLOBAL_PREFIX.set(prefix);
}

pub fn global_prefix() -> Option<&'static Path> {
    GLOBAL_PREFIX.get().map(PathBuf::as_path)
}

pub fn is_global() -> bool {
    GLOBAL_PREFIX.get().is_some()
}

/// 安装树：本地模式是 `<internal>/<产品>-<版本>`，全局模式就是 prefix。
pub fn tree_path(root: &Path, product: &str, version: &str) -> PathBuf {
    match global_prefix() {
        Some(prefix) => prefix.to_path_buf(),
        None => internal_dir(root).join(format!("{product}-{version}")),
    }
}

/// 状态文件：本地在 `<internal>/<产品>-<系列>/state.json`，全局在 prefix 里。
pub fn state_file(root: &Path, product: &str, series: &str) -> PathBuf {
    match global_prefix() {
        Some(prefix) => prefix.join(".phprt-state.json"),
        None => internal_dir(root)
            .join(format!("{product}-{series}"))
            .join("state.json"),
    }
}

/// 下载缓存：本地在 internal/downloads，全局用配置的目录。
pub fn downloads_dir(root: &Path) -> PathBuf {
    match DOWNLOAD_DIR.get() {
        Some(dir) => dir.clone(),
        None => internal_dir(root).join("downloads"),
    }
}

/// 全局模式的目录：`~` 展开，相对路径按 `$HOME` 解析。
pub fn resolve_home(value: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("读不到 HOME，无法定位全局目录"))?;
    let value = value.trim();
    if value == "~" {
        return Ok(home);
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return Ok(home.join(rest));
    }
    let path = Path::new(value);
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        home.join(path)
    })
}

/// 输出时优先显示相对项目根的路径，全局模式的绝对路径原样显示。
pub fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
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
    fn home_relative_paths_expand() {
        let home = std::env::var_os("HOME").expect("测试环境要有 HOME");
        assert_eq!(
            resolve_home("~/phprtbuild").unwrap(),
            PathBuf::from(&home).join("phprtbuild")
        );
        assert_eq!(
            resolve_home("data").unwrap(),
            PathBuf::from(&home).join("data")
        );
        assert_eq!(
            resolve_home("/data/build").unwrap(),
            PathBuf::from("/data/build")
        );
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
