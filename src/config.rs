use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use phprt_download::GitHubProxy;
use serde::Deserialize;

/// 下载配置文件，和 catalog.toml 同级放在 recipe 目录下。
pub const FILE_NAME: &str = "phprt.toml";

/// 项目根下的 `phprt.toml`：recipe 目录、工具目录和下载配置。文件不存在时用内置默认值。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_switch")]
    pub enable_github_proxy_download: String,
    #[serde(default = "default_proxy_urls")]
    pub proxy_urls: Vec<String>,
    /// 本地模式的工具目录（状态、安装树、缓存、备份），相对项目根；空表示 `.phprt`。
    /// 例：项目根 `/www/blog`，空 → `/www/blog/.phprt`，`data` → `/www/blog/data`。
    #[serde(default)]
    pub local_internal_dir: String,
    /// recipe 目录；空表示 `./recipes`。相对路径按项目根解析。
    #[serde(default)]
    pub recipes: String,
    /// 本地模式的源码解压目录；空表示 `<local_internal_dir>/build`。可绝对或相对。
    #[serde(default)]
    pub local_build_dir: String,
    /// 本地模式的组件登记文件备份目录；空表示用产品 recipe 的默认位置。
    #[serde(default)]
    pub local_backup_dir: String,
    /// 下载目录（两种模式共用）；空时本地用 internal/downloads、全局用 ~/phprtdownloads。
    #[serde(default)]
    pub download_dir: String,
    /// 全局模式的解压目录；空表示 `~/phprtbuild`。
    #[serde(default)]
    pub global_build_dir: String,
    /// 全局模式的备份目录；空表示 `~/phprtbackup`。
    #[serde(default)]
    pub global_backup_dir: String,
    /// 编译并发数；不写时取 CPU 数 - 1，最少 1。命令行 `--parallel` 优先。
    #[serde(default)]
    pub parallel: Option<usize>,
    /// 主线补丁检查的交互开关；不记录产品、版本或组件。
    #[serde(default)]
    pub catalog: CatalogConfig,
}

/// `[catalog]`：只放交互行为，不放数据源或组件选择。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogConfig {
    /// true 时在真正构建前询问一次；false 时不问、不查，直接用本地。
    #[serde(default = "default_true")]
    pub ask_before_online_update: bool,
    /// 交互式询问的默认答案，只能是 y 或 n。
    #[serde(default = "default_answer")]
    pub default_answer: String,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        Self {
            ask_before_online_update: true,
            default_answer: default_answer(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_answer() -> String {
    "y".to_owned()
}

/// 目录配置：空（或 `.`）视为没配置；去掉尾部 `/`；拒绝 `.`/`..` 段。
pub fn dir_value(key: &str, value: &str) -> Result<Option<String>> {
    let value = value.trim().trim_end_matches(['/', '\\']);
    if value.is_empty() {
        return Ok(None);
    }
    if value
        .split(['/', '\\'])
        .any(|part| part == "." || part == "..")
    {
        bail!("{FILE_NAME}: {key} 不能包含 . 或 .. 路径段");
    }
    Ok(Some(value.to_owned()))
}

fn default_switch() -> String {
    "enable".to_owned()
}

/// 没配 `parallel` 时用 CPU 数 - 1，最少 1：留一个核给前台。
pub fn default_parallel() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1))
        .unwrap_or(1)
        .max(1)
}

fn default_proxy_urls() -> Vec<String> {
    GitHubProxy::default().urls
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enable_github_proxy_download: default_switch(),
            proxy_urls: default_proxy_urls(),
            local_internal_dir: String::new(),
            recipes: String::new(),
            local_build_dir: String::new(),
            local_backup_dir: String::new(),
            download_dir: String::new(),
            global_build_dir: String::new(),
            global_backup_dir: String::new(),
            parallel: None,
            catalog: CatalogConfig::default(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            let config = Self::default();
            config.github_proxy()?;
            config.catalog_default_answer()?;
            return Ok(config);
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("读不到 {}", path.display()))?;
        let config: Self =
            toml::from_str(&text).with_context(|| format!("{} 不是合法的配置", path.display()))?;
        config.github_proxy()?;
        config.local_internal_dir_name()?;
        config.local_build_dir_value()?;
        config.local_backup_dir_name()?;
        config.download_dir_name()?;
        config.global_build_dir_name()?;
        config.global_backup_dir_name()?;
        config.parallel()?;
        config.catalog_default_answer()?;
        Ok(config)
    }

    /// 配置里的 recipe 目录；没配置或为空时返回 None。
    pub fn recipes_path(&self) -> Option<PathBuf> {
        let value = self.recipes.trim();
        if value.is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    }

    /// 本地模式的工具目录：空配置时是 `.phprt`；可绝对或相对（相对项目根）。
    pub fn local_internal_dir_name(&self) -> Result<String> {
        Ok(dir_value("local_internal_dir", &self.local_internal_dir)?
            .unwrap_or_else(|| crate::layout::DEFAULT_INTERNAL_DIR.to_owned()))
    }

    /// 本地模式显式配置的解压目录；None 表示跟随 `<local_internal_dir>/build`。
    /// 默认值由调用方用实际生效的 `local_internal_dir` 计算，见 `layout::local_build_dir_for`。
    pub fn local_build_dir_value(&self) -> Result<Option<String>> {
        dir_value("local_build_dir", &self.local_build_dir)
    }

    /// 本地模式的备份目录：空配置时为 None，用产品 recipe 的默认位置。
    pub fn local_backup_dir_name(&self) -> Result<Option<String>> {
        dir_value("local_backup_dir", &self.local_backup_dir)
    }

    /// 下载目录（两种模式共用）；空表示按模式用默认位置。
    pub fn download_dir_name(&self) -> Result<Option<String>> {
        dir_value("download_dir", &self.download_dir)
    }

    /// 全局模式解压目录；默认 `~/phprtbuild`。
    pub fn global_build_dir_name(&self) -> Result<String> {
        Ok(dir_value("global_build_dir", &self.global_build_dir)?
            .unwrap_or_else(|| "~/phprtbuild".to_owned()))
    }

    /// 全局模式备份目录；默认 `~/phprtbackup`。
    pub fn global_backup_dir_name(&self) -> Result<String> {
        Ok(dir_value("global_backup_dir", &self.global_backup_dir)?
            .unwrap_or_else(|| "~/phprtbackup".to_owned()))
    }

    /// 编译并发数：`make -j` 和 `CMAKE_BUILD_PARALLEL_LEVEL` 的值；未配置时 CPU 数 - 1，最少 1。
    pub fn parallel(&self) -> Result<usize> {
        match self.parallel {
            None => Ok(default_parallel()),
            Some(0) => bail!("{FILE_NAME}: parallel 至少为 1"),
            Some(n) => Ok(n),
        }
    }

    /// 交互式询问的默认答案；只接受 y / n。
    pub fn catalog_default_answer(&self) -> Result<bool> {
        match self
            .catalog
            .default_answer
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "y" => Ok(true),
            "n" => Ok(false),
            other => bail!("{FILE_NAME}: catalog.default_answer 只能是 y 或 n，实际是 {other}"),
        }
    }

    /// 校验开关并转换成下载器使用的设置。
    pub fn github_proxy(&self) -> Result<GitHubProxy> {
        let enabled = match self.enable_github_proxy_download.trim() {
            "enable" => true,
            "disable" => false,
            other => bail!(
                "{FILE_NAME}: enable_github_proxy_download 只能是 enable 或 disable，实际是 {other}"
            ),
        };
        Ok(GitHubProxy {
            enabled,
            urls: self.proxy_urls.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_uses_builtin_defaults() {
        let config = Config::load(Path::new("/nonexistent/phprt.toml")).unwrap();
        let proxy = config.github_proxy().unwrap();
        assert!(proxy.enabled);
        assert_eq!(proxy.urls.len(), 4);
    }

    #[test]
    fn disable_turns_off_mirrors() {
        let config: Config = toml::from_str(
            "enable_github_proxy_download = \"disable\"\nproxy_urls = [\"https://gh-proxy.com/\"]\n",
        )
        .unwrap();
        assert!(!config.github_proxy().unwrap().enabled);
    }

    #[test]
    fn parallel_defaults_to_cpus_minus_one() {
        let config = Config::load(Path::new("/nonexistent/phprt.toml")).unwrap();
        assert_eq!(config.parallel().unwrap(), default_parallel());
        // 单核机器上 -1 会变成 0，必须兜到 1。
        assert!(config.parallel().unwrap() >= 1);
    }

    #[test]
    fn parallel_accepts_explicit_value() {
        let config: Config = toml::from_str("parallel = 6\n").unwrap();
        assert_eq!(config.parallel().unwrap(), 6);
    }

    #[test]
    fn zero_parallel_is_an_error() {
        let config: Config = toml::from_str("parallel = 0\n").unwrap();
        let err = config.parallel().unwrap_err().to_string();
        assert!(err.contains("parallel"), "{err}");
    }

    #[test]
    fn internal_dir_defaults_to_dot_phprt() {
        let config = Config::load(Path::new("/nonexistent/phprt.toml")).unwrap();
        assert_eq!(config.local_internal_dir_name().unwrap(), ".phprt");

        let config: Config = toml::from_str("local_internal_dir = \"\"\n").unwrap();
        assert_eq!(config.local_internal_dir_name().unwrap(), ".phprt");

        let config: Config = toml::from_str("local_internal_dir = \"data\"\n").unwrap();
        assert_eq!(config.local_internal_dir_name().unwrap(), "data");
    }

    #[test]
    fn dirs_accept_paths_but_not_dot_dot() {
        // 绝对路径和带子目录的相对路径都允许。
        for ok in ["/opt/php", "data/phprt", ".phprt"] {
            let config: Config = toml::from_str(&format!("local_internal_dir = '{ok}'\n")).unwrap();
            assert_eq!(config.local_internal_dir_name().unwrap(), ok);
        }
        for bad in ["../x", "a/../../b", ".", "..", "/opt/../x"] {
            let config: Config =
                toml::from_str(&format!("local_internal_dir = '{bad}'\n")).unwrap();
            assert!(config.local_internal_dir_name().is_err(), "{bad}");
        }

        let config: Config =
            toml::from_str("local_build_dir = '/tmp/phprt-build'\nlocal_backup_dir = 'backups'\n")
                .unwrap();
        assert_eq!(
            config.local_build_dir_value().unwrap().as_deref(),
            Some("/tmp/phprt-build")
        );
        assert_eq!(
            config.local_backup_dir_name().unwrap().as_deref(),
            Some("backups")
        );
        assert_eq!(Config::default().local_backup_dir_name().unwrap(), None);
        assert_eq!(Config::default().local_build_dir_value().unwrap(), None);

        // 没配 local_build_dir 时跟随工具目录。
        let internal = Config::default().local_internal_dir_name().unwrap();
        assert_eq!(
            crate::layout::local_build_dir_for(&internal),
            ".phprt/build"
        );
        let config: Config = toml::from_str("local_internal_dir = 'data'\n").unwrap();
        let internal = config.local_internal_dir_name().unwrap();
        assert_eq!(crate::layout::local_build_dir_for(&internal), "data/build");
    }

    #[test]
    fn global_dirs_default_to_home_names() {
        let config = Config::default();
        assert_eq!(config.download_dir_name().unwrap(), None);
        assert_eq!(config.global_build_dir_name().unwrap(), "~/phprtbuild");
        assert_eq!(config.global_backup_dir_name().unwrap(), "~/phprtbackup");

        let config: Config = toml::from_str(
            "download_dir = '/data/downloads'\nglobal_build_dir = '/data/build'\nglobal_backup_dir = 'backups'\n",
        )
        .unwrap();
        assert_eq!(
            config.download_dir_name().unwrap().as_deref(),
            Some("/data/downloads")
        );
        assert_eq!(config.global_build_dir_name().unwrap(), "/data/build");
        assert_eq!(config.global_backup_dir_name().unwrap(), "backups");
    }

    #[test]
    fn unknown_config_key_is_rejected() {
        let err = toml::from_str::<Config>("internal_dir = 'x'\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown field"), "{err}");
    }

    #[test]
    fn unknown_switch_is_rejected() {
        let config: Config = toml::from_str("enable_github_proxy_download = \"yes\"\n").unwrap();
        assert!(config.github_proxy().is_err());
    }

    #[test]
    fn catalog_defaults_to_asking_with_yes() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.catalog.ask_before_online_update);
        assert!(config.catalog_default_answer().unwrap());
    }

    #[test]
    fn catalog_answer_and_switch_are_validated() {
        let config: Config =
            toml::from_str("[catalog]\nask_before_online_update = false\ndefault_answer = \"n\"\n")
                .unwrap();
        assert!(!config.catalog.ask_before_online_update);
        assert!(!config.catalog_default_answer().unwrap());

        let config: Config = toml::from_str("[catalog]\ndefault_answer = \"yes\"\n").unwrap();
        let err = config.catalog_default_answer().unwrap_err().to_string();
        assert!(err.contains("default_answer"), "{err}");
    }

    #[test]
    fn unknown_catalog_key_is_an_error() {
        let err = toml::from_str::<Config>("[catalog]\nprompt = false\n").unwrap_err();
        assert!(err.to_string().contains("prompt"), "{err}");
    }
}
