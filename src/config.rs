use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use phprt_download::GitHubProxy;
use serde::Deserialize;

/// 下载配置文件，和 catalog.toml 同级放在 recipe 目录下。
pub const FILE_NAME: &str = "phprt.toml";

/// `recipes/phprt.toml`：下载行为配置。文件不存在时用内置默认值。
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_switch")]
    pub enable_github_proxy_download: String,
    #[serde(default = "default_proxy_urls")]
    pub proxy_urls: Vec<String>,
}

fn default_switch() -> String {
    "enable".to_owned()
}

fn default_proxy_urls() -> Vec<String> {
    GitHubProxy::default().urls
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enable_github_proxy_download: default_switch(),
            proxy_urls: default_proxy_urls(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            let config = Self::default();
            config.github_proxy()?;
            return Ok(config);
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("读不到 {}", path.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("{} 不是合法的下载配置", path.display()))?;
        config.github_proxy()?;
        Ok(config)
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
    fn unknown_switch_is_rejected() {
        let config: Config = toml::from_str("enable_github_proxy_download = \"yes\"\n").unwrap();
        assert!(config.github_proxy().is_err());
    }
}
