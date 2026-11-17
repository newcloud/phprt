use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use phprt_download::GitHubProxy;
use serde::Deserialize;

/// 下载配置文件，和 catalog.toml 同级放在 recipe 目录下。
pub const FILE_NAME: &str = "phprt.toml";

/// 项目根下的 `phprt.toml`：recipe 目录、工具目录和下载配置。文件不存在时用内置默认值。
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_switch")]
    pub enable_github_proxy_download: String,
    #[serde(default = "default_proxy_urls")]
    pub proxy_urls: Vec<String>,
    /// 工具目录名（状态、安装树、缓存、备份），相对项目根；空表示 `.phprt`。
    /// 例：项目根 `/www/blog`，空 → `/www/blog/.phprt`，`data` → `/www/blog/data`。
    #[serde(default)]
    pub internal_dir: String,
    /// recipe 目录；空表示 `./recipes`。相对路径按项目根解析。
    #[serde(default)]
    pub recipes: String,
    /// 源码解压目录；空表示 `build`。可绝对或相对。
    #[serde(default)]
    pub build_dir: String,
    /// 组件登记文件备份目录；空表示用产品 recipe 的默认位置（internal_dir/backup）。
    #[serde(default)]
    pub backup_dir: String,
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

fn default_proxy_urls() -> Vec<String> {
    GitHubProxy::default().urls
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enable_github_proxy_download: default_switch(),
            proxy_urls: default_proxy_urls(),
            internal_dir: String::new(),
            recipes: String::new(),
            build_dir: String::new(),
            backup_dir: String::new(),
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
        let config: Self =
            toml::from_str(&text).with_context(|| format!("{} 不是合法的配置", path.display()))?;
        config.github_proxy()?;
        config.internal_dir_name()?;
        config.build_dir_name()?;
        config.backup_dir_name()?;
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

    /// 工具目录：空配置时是 `.phprt`；可绝对或相对（相对项目根）。
    pub fn internal_dir_name(&self) -> Result<String> {
        Ok(dir_value("internal_dir", &self.internal_dir)?
            .unwrap_or_else(|| crate::layout::DEFAULT_INTERNAL_DIR.to_owned()))
    }

    /// 解压目录：空配置时是 `build`；可绝对或相对（相对项目根）。
    pub fn build_dir_name(&self) -> Result<String> {
        Ok(dir_value("build_dir", &self.build_dir)?
            .unwrap_or_else(|| crate::layout::DEFAULT_BUILD_DIR.to_owned()))
    }

    /// 备份目录：空配置时为 None，用产品 recipe 的默认位置。
    pub fn backup_dir_name(&self) -> Result<Option<String>> {
        dir_value("backup_dir", &self.backup_dir)
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
    fn internal_dir_defaults_to_dot_phprt() {
        let config = Config::load(Path::new("/nonexistent/phprt.toml")).unwrap();
        assert_eq!(config.internal_dir_name().unwrap(), ".phprt");

        let config: Config = toml::from_str("internal_dir = \"\"\n").unwrap();
        assert_eq!(config.internal_dir_name().unwrap(), ".phprt");

        let config: Config = toml::from_str("internal_dir = \"data\"\n").unwrap();
        assert_eq!(config.internal_dir_name().unwrap(), "data");
    }

    #[test]
    fn dirs_accept_paths_but_not_dot_dot() {
        // 绝对路径和带子目录的相对路径都允许。
        for ok in ["/opt/php", "data/phprt", ".phprt"] {
            let config: Config = toml::from_str(&format!("internal_dir = '{ok}'\n")).unwrap();
            assert_eq!(config.internal_dir_name().unwrap(), ok);
        }
        for bad in ["../x", "a/../../b", ".", "..", "/opt/../x"] {
            let config: Config = toml::from_str(&format!("internal_dir = '{bad}'\n")).unwrap();
            assert!(config.internal_dir_name().is_err(), "{bad}");
        }

        let config: Config =
            toml::from_str("build_dir = '/tmp/phprt-build'\nbackup_dir = 'backups'\n").unwrap();
        assert_eq!(config.build_dir_name().unwrap(), "/tmp/phprt-build");
        assert_eq!(
            config.backup_dir_name().unwrap().as_deref(),
            Some("backups")
        );
        assert_eq!(Config::default().backup_dir_name().unwrap(), None);
        assert_eq!(Config::default().build_dir_name().unwrap(), "build");
    }

    #[test]
    fn unknown_switch_is_rejected() {
        let config: Config = toml::from_str("enable_github_proxy_download = \"yes\"\n").unwrap();
        assert!(config.github_proxy().is_err());
    }
}
