//! 宿主缺命令：包名映射、包管理器识别、安装命令。
//!
//! 只提示，不执行：不跑 `sudo`、不装包、不改 `PATH`。包名映射是数据，
//! 放在 `recipes/images.toml` 的 `[package_maps.*]`，发行版改包名不用重编 phprt。

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::tools;

/// 支持的两个包管理器；镜像模式也用这两个名字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pm {
    Apt,
    Dnf,
}

impl Pm {
    pub fn as_str(self) -> &'static str {
        match self {
            Pm::Apt => "apt",
            Pm::Dnf => "dnf",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "apt" => Some(Pm::Apt),
            "dnf" => Some(Pm::Dnf),
            _ => None,
        }
    }

    /// 宿主提示用的安装命令；镜像模式在容器里不带 `sudo`，模板另写。
    fn install_prefix(self) -> &'static str {
        match self {
            Pm::Apt => "sudo apt-get install",
            Pm::Dnf => "sudo dnf install",
        }
    }
}

/// `recipes/images.toml` 里的包名映射。
#[derive(Debug, Clone, Default)]
struct Maps {
    apt: BTreeMap<String, String>,
    dnf: BTreeMap<String, String>,
}

impl Maps {
    fn get(&self, pm: Pm, command: &str) -> Option<&str> {
        let table = match pm {
            Pm::Apt => &self.apt,
            Pm::Dnf => &self.dnf,
        };
        table.get(command).map(String::as_str)
    }
}

/// 宿主环境：识别出的包管理器、系统名、包名映射。
#[derive(Debug, Clone, Default)]
pub struct Env {
    pm: Option<Pm>,
    /// 显示用的系统名，如 `Fedora Linux 43`；识别不出时为空。
    os: String,
    /// `/etc/os-release` 的 `ID`，小写，如 `fedora` / `debian`；识别不出时为空。
    id: String,
    maps: Maps,
}

impl Env {
    /// 读 `recipes/images.toml` 的 `[package_maps.*]`，并识别本机发行版。
    ///
    /// 文件不存在时映射为空，行为等同“全部未映射”，不报错。
    pub fn load(recipes: &Path) -> Result<Self> {
        let (pm, os, id) = detect();
        let maps = load_maps(&recipes.join("images.toml"))?;
        Ok(Self { pm, os, id, maps })
    }

    /// 发行版家族：只分 `rpm` / `deb`，认不出时为空。
    ///
    /// recipe 里按它选跟系统路径有关的构建参数（例如 `--openssldir`）。
    pub fn family(&self) -> &'static str {
        match self.pm {
            Some(Pm::Apt) => "deb",
            Some(Pm::Dnf) => "rpm",
            None => "",
        }
    }

    /// 注入 recipe 的宿主事实：`ctx["host"]`。
    pub fn recipe_ctx(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "family": self.family(),
        })
    }

    /// 单条命令的安装命令；未映射或识别不出包管理器时为空。
    fn hint(&self, command: &str) -> Option<String> {
        let pm = self.pm?;
        let package = self.maps.get(pm, command)?;
        Some(format!("{} {package}", pm.install_prefix()))
    }

    fn os_label(&self) -> String {
        label(&self.os, self.pm.map(Pm::as_str))
    }

    /// 汇总本次缺的命令；没有缺命令时返回 `None`。
    ///
    /// `toolchain` 是编译器命令名（按声明顺序在前），`commands` 是工具解析里
    /// 缺命令的名字（去重后按字典序），`blocked` 是不能靠装包解决的原因。
    pub fn report(
        &self,
        toolchain: &[String],
        commands: &[String],
        reasons: BTreeMap<String, String>,
        blocked: Vec<String>,
    ) -> Option<Report> {
        let mut missing: Vec<String> = Vec::new();
        for name in toolchain {
            if !missing.contains(name) {
                missing.push(name.clone());
            }
        }
        let mut rest: Vec<String> = commands
            .iter()
            .filter(|name| !missing.contains(name))
            .cloned()
            .collect();
        rest.sort();
        rest.dedup();
        missing.extend(rest);
        if missing.is_empty() {
            return None;
        }

        let mut packages: Vec<String> = Vec::new();
        let mut unmapped: Vec<String> = Vec::new();
        if let Some(pm) = self.pm {
            for command in &missing {
                match self.maps.get(pm, command) {
                    Some(package) => {
                        if !packages.iter().any(|item| item == package) {
                            packages.push(package.to_owned());
                        }
                    }
                    None => unmapped.push(command.clone()),
                }
            }
        }
        let install = match self.pm {
            Some(pm) if !packages.is_empty() => {
                format!("{} {}", pm.install_prefix(), packages.join(" "))
            }
            _ => String::new(),
        };
        // 说明只保留这次真的缺的命令，避免历史原因串到别的提示里。
        let reasons = reasons
            .into_iter()
            .filter(|(name, reason)| !reason.is_empty() && missing.contains(name))
            .collect();
        Some(Report {
            pm: self.pm.map(Pm::as_str).unwrap_or_default().to_owned(),
            os: self.os.clone(),
            missing,
            reasons,
            unmapped,
            install,
            blocked,
        })
    }
}

/// 本次缺命令的结果；文本和 JSON 都从这里出。
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// 包管理器；识别不出时为空。
    pub pm: String,
    /// 缺的命令名，编译器在前，其余按字典序。
    pub missing: Vec<String>,
    /// 命令键 -> 一句人话说明（工具 recipe 的 `need_packages(..., reason = ...)`）。
    pub reasons: BTreeMap<String, String>,
    /// 有命令但映射表里没有包名的项。
    pub unmapped: Vec<String>,
    /// 拼好的安装命令；识别不出包管理器或没有可映射的名字时为空。
    pub install: String,
    #[serde(skip)]
    os: String,
    /// 不能靠装包解决的原因（版本不满足、catalog 数据问题）。
    #[serde(skip)]
    blocked: Vec<String>,
}

impl Report {
    /// `install` / `add` / `remove` 的聚合错误正文，不带 `error: ` 前缀。
    pub fn error_message(&self) -> String {
        let mut out = format!("宿主缺少构建命令：{}", self.missing.join(" "));
        out.push_str(
            "\n这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。",
        );
        for (name, reason) in &self.reasons {
            out.push_str(&format!("\n  {name}：{reason}"));
        }
        if !self.install.is_empty() {
            out.push_str(&format!(
                "\n{}上执行：\n  {}",
                self.os_label(),
                self.install
            ));
        } else if self.pm.is_empty() {
            out.push_str("\n未识别发行版，请用本机包管理器安装以上命令。");
        }
        if !self.unmapped.is_empty() {
            out.push_str(&format!(
                "\n未映射：{}（请自行确认包名）",
                self.unmapped.join(" ")
            ));
        }
        if !self.blocked.is_empty() {
            out.push_str(&format!("\n另有 {} 项不能靠装包解决：", self.blocked.len()));
            for reason in &self.blocked {
                out.push_str(&format!("\n  {reason}"));
            }
        }
        out
    }

    /// `plan` 文本里 `install` 那一行的值。
    pub fn plan_install(&self) -> &str {
        if !self.install.is_empty() {
            return &self.install;
        }
        if self.pm.is_empty() {
            "未识别发行版，请用本机包管理器安装"
        } else {
            "-"
        }
    }

    fn os_label(&self) -> String {
        label(&self.os, (!self.pm.is_empty()).then_some(self.pm.as_str()))
    }
}

/// `Fedora Linux 43（dnf）`；系统名或包管理器缺失时只写有的那一段。
fn label(os: &str, pm: Option<&str>) -> String {
    match (os.is_empty(), pm) {
        (false, Some(pm)) => format!("{os}（{pm}）"),
        (false, None) => os.to_owned(),
        (true, Some(pm)) => pm.to_owned(),
        (true, None) => String::new(),
    }
}

/// `feature_available` 的单命令提示：命令名 + 该发行版的安装命令。
pub fn feature_hint(env: &Env, target: &str, feature: &str, command: &str) -> String {
    let mut out = format!("{target} 的 feature {feature} 需要命令 {command}");
    if let Some(install) = env.hint(command) {
        out.push_str(&format!("\n{}上执行：\n  {install}", env.os_label()));
    }
    out
}

fn load_maps(path: &Path) -> Result<Maps> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Maps::default()),
        Err(err) => return Err(err).with_context(|| format!("读不到 {}", path.display())),
    };
    let file: ImagesFile =
        toml::from_str(&text).with_context(|| format!("{}: TOML 解析失败", path.display()))?;
    let mut maps = Maps::default();
    for (name, table) in file.package_maps {
        let pm = Pm::parse(&name).ok_or_else(|| {
            anyhow::anyhow!(
                "{}: package_maps 只支持 apt / dnf，实际是 {name}",
                path.display()
            )
        })?;
        for (command, package) in table {
            if package.trim().is_empty() {
                bail!(
                    "{}: package_maps.{name}.{command} 的包名不能为空",
                    path.display()
                );
            }
            match pm {
                Pm::Apt => {
                    maps.apt.insert(command, package);
                }
                Pm::Dnf => {
                    maps.dnf.insert(command, package);
                }
            }
        }
    }
    Ok(maps)
}

#[derive(Deserialize, Default)]
struct ImagesFile {
    #[serde(default)]
    package_maps: BTreeMap<String, BTreeMap<String, String>>,
}

/// 先读 `/etc/os-release`（没有就读 `/usr/lib/os-release`），得不出结论时看 PATH。
///
/// 返回（包管理器、显示名、发行版 ID）。
fn detect() -> (Option<Pm>, String, String) {
    let text = fs::read_to_string("/etc/os-release")
        .or_else(|_| fs::read_to_string("/usr/lib/os-release"))
        .ok();
    let fields = text.as_deref().map(parse_os_release).unwrap_or_default();
    let os = display_name(&fields);
    let id = release_id(&fields);
    if let Some(pm) = pm_from_release(&fields) {
        return (Some(pm), os, id);
    }
    if tools::on_path("apt-get") {
        return (Some(Pm::Apt), os, id);
    }
    if tools::on_path("dnf") {
        return (Some(Pm::Dnf), os, id);
    }
    (None, os, id)
}

/// `ID=fedora` -> `fedora`；没有 ID 时为空。
fn release_id(fields: &BTreeMap<String, String>) -> String {
    fields
        .get("ID")
        .map(|value| value.trim().to_lowercase())
        .unwrap_or_default()
}

fn pm_from_release(fields: &BTreeMap<String, String>) -> Option<Pm> {
    let mut tokens: Vec<String> = Vec::new();
    if let Some(id) = fields.get("ID") {
        tokens.push(id.to_lowercase());
    }
    if let Some(like) = fields.get("ID_LIKE") {
        tokens.extend(like.split_whitespace().map(str::to_lowercase));
    }
    for token in &tokens {
        if matches!(
            token.as_str(),
            "debian" | "ubuntu" | "linuxmint" | "pop" | "raspbian"
        ) {
            return Some(Pm::Apt);
        }
        if matches!(
            token.as_str(),
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol"
        ) {
            return Some(Pm::Dnf);
        }
    }
    None
}

pub(crate) fn parse_os_release(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .unwrap_or(value);
        out.insert(key.trim().to_owned(), value.to_owned());
    }
    out
}

fn display_name(fields: &BTreeMap<String, String>) -> String {
    let name = fields
        .get("NAME")
        .or_else(|| fields.get("ID"))
        .cloned()
        .unwrap_or_default();
    match fields.get("VERSION_ID") {
        Some(version) if !name.is_empty() => format!("{name} {version}"),
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with(pm: Option<Pm>, os: &str, dnf: &[(&str, &str)]) -> Env {
        let mut maps = Maps::default();
        for (command, package) in dnf {
            maps.dnf
                .insert((*command).to_owned(), (*package).to_owned());
        }
        Env {
            pm,
            os: os.to_owned(),
            id: String::new(),
            maps,
        }
    }

    fn fedora() -> Env {
        env_with(
            Some(Pm::Dnf),
            "Fedora Linux 43",
            &[
                ("gcc", "gcc"),
                ("g++", "gcc-c++"),
                ("make", "make"),
                ("cmake", "cmake"),
                ("pkg-config", "pkgconf"),
                ("perl", "perl"),
            ],
        )
    }

    #[test]
    fn release_picks_the_package_manager() {
        let debian = parse_os_release("ID=ubuntu\nID_LIKE=debian\n");
        assert_eq!(pm_from_release(&debian), Some(Pm::Apt));
        assert_eq!(release_id(&debian), "ubuntu");

        let fedora = parse_os_release("ID=fedora\n");
        assert_eq!(pm_from_release(&fedora), Some(Pm::Dnf));

        let rhel = parse_os_release("ID=\"rocky\"\nID_LIKE=\"rhel centos\"\n");
        assert_eq!(pm_from_release(&rhel), Some(Pm::Dnf));

        let unknown = parse_os_release("ID=arch\n");
        assert_eq!(pm_from_release(&unknown), None);
    }

    #[test]
    fn release_values_are_unquoted_and_displayed_as_name_plus_version() {
        let fields = parse_os_release(
            "NAME=\"Fedora Linux\"\nVERSION_ID=\"43\"\nPRETTY_NAME=\"Fedora Linux 43 (Workstation)\"\n",
        );
        assert_eq!(fields["NAME"], "Fedora Linux");
        assert_eq!(display_name(&fields), "Fedora Linux 43");
        assert_eq!(release_id(&fields), "");
    }

    #[test]
    fn host_ctx_carries_id_and_family() {
        let env = Env {
            pm: Some(Pm::Apt),
            os: "Debian GNU/Linux 13".to_owned(),
            id: "debian".to_owned(),
            maps: Maps::default(),
        };
        assert_eq!(env.family(), "deb");
        assert_eq!(env.recipe_ctx()["id"], "debian");
        assert_eq!(env.recipe_ctx()["family"], "deb");

        let rpm = Env {
            pm: Some(Pm::Dnf),
            os: "Fedora Linux 43".to_owned(),
            id: "fedora".to_owned(),
            maps: Maps::default(),
        };
        assert_eq!(rpm.recipe_ctx()["family"], "rpm");

        // 认不出发行版时家族为空，recipe 退回不依赖系统的默认值。
        let unknown = Env::default();
        assert_eq!(unknown.family(), "");
        assert_eq!(unknown.recipe_ctx()["family"], "");
    }

    #[test]
    fn maps_are_keyed_by_command_name() {
        let env = fedora();
        assert_eq!(env.maps.get(Pm::Dnf, "g++"), Some("gcc-c++"));
        assert_eq!(env.maps.get(Pm::Dnf, "pkg-config"), Some("pkgconf"));
        assert_eq!(env.maps.get(Pm::Dnf, "nope"), None);
    }

    #[test]
    fn report_puts_the_compilers_first_then_sorted_commands() {
        let env = fedora();
        let report = env
            .report(
                &["gcc".to_owned(), "g++".to_owned()],
                &[
                    "make".to_owned(),
                    "cmake".to_owned(),
                    "make".to_owned(),
                    "g++".to_owned(),
                ],
                BTreeMap::new(),
                Vec::new(),
            )
            .unwrap();
        assert_eq!(report.missing, ["gcc", "g++", "cmake", "make"]);
        assert_eq!(
            report.install,
            "sudo dnf install gcc gcc-c++ cmake make".to_owned()
        );
        assert_eq!(
            report.error_message(),
            "宿主缺少构建命令：gcc g++ cmake make\n\
             这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。\n\
             Fedora Linux 43（dnf）上执行：\n  \
             sudo dnf install gcc gcc-c++ cmake make"
        );
    }

    #[test]
    fn reasons_are_printed_after_the_missing_line() {
        let env = fedora();
        let report = env
            .report(
                &[],
                &["make".to_owned(), "perl".to_owned()],
                BTreeMap::from([("perl".to_owned(), "当前 perl 模块不全".to_owned())]),
                Vec::new(),
            )
            .unwrap();
        assert_eq!(report.reasons["perl"], "当前 perl 模块不全");
        assert_eq!(
            report.error_message(),
            "宿主缺少构建命令：make perl\n\
             这些命令 catalog 里没有可用的源码 recipe，phprt 不会自己编译；由系统包提供。\n  \
             perl：当前 perl 模块不全\n\
             Fedora Linux 43（dnf）上执行：\n  \
             sudo dnf install make perl"
        );
    }

    #[test]
    fn reasons_for_commands_that_are_not_missing_are_dropped() {
        let env = fedora();
        let report = env
            .report(
                &[],
                &["make".to_owned()],
                BTreeMap::from([("perl".to_owned(), "当前 perl 模块不全".to_owned())]),
                Vec::new(),
            )
            .unwrap();
        assert!(report.reasons.is_empty());
        assert!(!report.error_message().contains("perl"));
    }

    #[test]
    fn no_missing_command_means_no_report() {
        let env = fedora();
        assert!(env.report(&[], &[], BTreeMap::new(), Vec::new()).is_none());
    }

    #[test]
    fn unmapped_commands_are_listed_but_not_installed() {
        let env = fedora();
        let report = env
            .report(&[], &["foo".to_owned()], BTreeMap::new(), Vec::new())
            .unwrap();
        assert_eq!(report.missing, ["foo"]);
        assert_eq!(report.unmapped, ["foo"]);
        assert_eq!(report.install, "");
        let message = report.error_message();
        assert!(
            message.contains("未映射：foo（请自行确认包名）"),
            "{message}"
        );
        assert!(!message.contains("sudo dnf install"), "{message}");
    }

    #[test]
    fn unknown_package_manager_lists_names_only() {
        let env = env_with(None, "", &[]);
        let report = env
            .report(&[], &["make".to_owned()], BTreeMap::new(), Vec::new())
            .unwrap();
        assert_eq!(report.pm, "");
        assert_eq!(report.install, "");
        assert_eq!(report.plan_install(), "未识别发行版，请用本机包管理器安装");
        let message = report.error_message();
        assert!(
            message.contains("未识别发行版，请用本机包管理器安装以上命令。"),
            "{message}"
        );
    }

    #[test]
    fn blocked_items_are_appended_after_the_install_command() {
        let env = fedora();
        let report = env
            .report(
                &[],
                &["make".to_owned()],
                BTreeMap::new(),
                vec!["cmake 3.10 不满足 >= 3.20，catalog 也没有可用版本".to_owned()],
            )
            .unwrap();
        let message = report.error_message();
        assert!(message.contains("sudo dnf install make"), "{message}");
        assert!(
            message.contains("另有 1 项不能靠装包解决：\n  cmake 3.10 不满足"),
            "{message}"
        );
    }

    #[test]
    fn feature_hint_uses_the_same_generator() {
        let env = fedora();
        let hint = feature_hint(&env, "dep:jpeg", "simd", "make");
        assert_eq!(
            hint,
            "dep:jpeg 的 feature simd 需要命令 make\n\
             Fedora Linux 43（dnf）上执行：\n  \
             sudo dnf install make"
        );
    }

    #[test]
    fn package_maps_only_accept_apt_and_dnf() {
        let dir = std::env::temp_dir().join(format!("phprt-host-commands-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("images.toml");
        std::fs::write(&path, "[package_maps.pacman]\nmake = \"make\"\n").unwrap();
        let err = load_maps(&path).unwrap_err().to_string();
        assert!(err.contains("只支持 apt / dnf"), "{err}");

        std::fs::write(&path, "[package_maps.dnf]\nmake = \"\"\n").unwrap();
        let err = load_maps(&path).unwrap_err().to_string();
        assert!(err.contains("包名不能为空"), "{err}");

        std::fs::write(&path, "[package_maps.dnf]\nmake = \"make\"\n").unwrap();
        let maps = load_maps(&path).unwrap();
        assert_eq!(maps.get(Pm::Dnf, "make"), Some("make"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
