use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use starlark::environment::{Globals, Module};
use starlark::eval::Evaluator;
use starlark::syntax::{AstModule, Dialect};

use crate::catalog::Catalog;

/// 三层目标：产品、组件、依赖库。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Dep,
    Component,
    Product,
    Tool,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Dep => "dep",
            Kind::Component => "component",
            Kind::Product => "product",
            Kind::Tool => "tool",
        }
    }
}

/// 挂载策略：fold 只把参数/源码交给产品，standalone 在产品之后单独构建。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Fold,
    Standalone,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Feature {
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub configure: Vec<String>,
    /// feature 未启用时应用的参数（关闭上游自动探测）。
    #[serde(default)]
    pub disable: Vec<String>,
    /// feature 的可用条件；dep feature 只允许 commands。
    #[serde(default)]
    pub requires: Requires,
}

/// feature 的可用条件：允许的产品系列、允许的依赖线、宿主机命令。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Requires {
    #[serde(default)]
    pub product: Vec<String>,
    #[serde(default)]
    pub dep: BTreeMap<String, Vec<String>>,
    /// 需要出现在 PATH 里的宿主机命令，如 `nasm`。
    #[serde(default)]
    pub commands: Vec<String>,
}

/// 产品的一种组件挂载方式。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileMode {
    /// `fold` 或 `standalone`。
    #[serde(default)]
    pub strategy: String,
    /// standalone 的步骤模板；支持 `{flags}` / `{name}` / `{product}` / `{build}`。
    #[serde(default)]
    pub steps: Vec<String>,
    /// standalone / fold 追加到产品的产物模式；支持 `{name}`。
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// fold 追加到产品 configure 的参数模板；支持 `{component_source}` / `{name}`。
    #[serde(default)]
    pub configure: Vec<String>,
}

/// 产品安装树的运行时整理规则。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Runtime {
    /// 依赖库的统一运行目录，相对安装树；空表示不做运行库收集。
    #[serde(default)]
    pub lib_dir: String,
    /// 安装树内的 rpath 规则：相对路径 glob -> rpath。
    #[serde(default)]
    pub rpath: BTreeMap<String, String>,
}

impl ProfileMode {
    pub fn strategy_kind(&self) -> Result<Strategy> {
        match self.strategy.as_str() {
            "fold" => Ok(Strategy::Fold),
            "standalone" => Ok(Strategy::Standalone),
            other => bail!("strategy={other} 不合法，只能是 fold 或 standalone"),
        }
    }
}

impl Strategy {
    pub fn as_str(self) -> &'static str {
        match self {
            Strategy::Fold => "fold",
            Strategy::Standalone => "standalone",
        }
    }
}

/// 组件登记文件的备份规则，来自产品 profile 的 `backup(relative_dir, name)`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPolicy {
    /// 相对工具目录（默认 `.phprt`，构建后的安装树所在），如 `backup`。
    pub relative_dir: String,
    /// 文件名模板，支持 `{timestamp}` / `{component}` / `{mode}`。
    pub name: String,
}

impl BackupPolicy {
    fn validate(&self, at: &str) -> Result<()> {
        if self.relative_dir.is_empty() {
            bail!("{at}: profile.backup.relative_dir 不能为空");
        }
        check_relative(&self.relative_dir, "profile.backup.relative_dir")?;
        if self.name.is_empty() {
            bail!("{at}: profile.backup.name 不能为空");
        }
        if self.name.contains('/') || self.name.contains('\\') {
            bail!("{at}: profile.backup.name 不能带路径分隔符");
        }
        if !self.name.contains("{component}") {
            bail!("{at}: profile.backup.name 必须包含 {{component}}");
        }
        let mut rest = self.name.as_str();
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start..].find('}').map(|i| i + start) else {
                bail!("{at}: profile.backup.name 里的占位符没有闭合");
            };
            let key = &rest[start + 1..end];
            if !matches!(key, "timestamp" | "component" | "mode") {
                bail!("{at}: profile.backup.name 里的占位符 {{{key}}} 不支持");
            }
            rest = &rest[end + 1..];
        }
        Ok(())
    }
}

/// 产品 profile：组件挂载协议、运行时整理、验证和入口。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub modes: BTreeMap<String, ProfileMode>,
    /// 产品构建总是需要的宿主机工具。
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    /// 安装后动作，按顺序执行。
    #[serde(default)]
    pub post_install: Vec<Action>,
    /// 组件来源 -> 默认挂载方式，如 `{ bundled = "static", external = "shared" }`。
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
    /// bundled 组件在产品源码树里的默认相对路径模板，如 `ext/{name}`。
    #[serde(default)]
    pub bundled_path: String,
    #[serde(default)]
    pub runtime: Runtime,
    /// 挂载方式 -> 登记动作名；由 post_install 的 write_component_ini 消费。
    #[serde(default)]
    pub register: BTreeMap<String, String>,
    /// 组件登记文件的备份规则；不写就不备份。
    #[serde(default)]
    pub backup: Option<BackupPolicy>,
    #[serde(default)]
    pub verify: Vec<String>,
    #[serde(default)]
    pub verify_modules: String,
    /// 入口名模板，如 `myphp-{series}`；空表示不生成入口。
    #[serde(default)]
    pub entry: String,
    /// 入口模板文件，相对产品 recipe 目录，如 `entry.sh`。
    #[serde(default)]
    pub entry_template: String,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 选项来源：`php-config-m4` / `configure-help` / `none`。
    #[serde(default)]
    pub audit: String,
}

/// 工具的一个具名检查：命令存在且版本满足后再执行，退出码 0 通过。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureCheck {
    /// 检查命令的参数（不带命令本身），如 `["-MFindBin", "-e1"]`。
    pub run: Vec<String>,
    /// 检查失败时怎么处理。
    pub on_fail: CheckFail,
}

/// 检查失败的处理方式；`need_packages` 进统一安装提示，`error_msg` 直接报错。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckFail {
    /// 这些命令键需要装包；Rust 侧走 [package_maps.*] 合并成一条安装命令。
    NeedPackages {
        names: Vec<String>,
        /// 给人看的说明，写在装包提示后面；空表示不写。
        #[serde(default)]
        reason: String,
    },
    /// 装包解决不了，直接打印这段文字。
    ErrorMsg { text: String },
}

/// 一个宿主机工具要求：字符串缩写只检查存在，带 min / max 时检查版本区间，
/// `feature_checks` 是要在这条命令上执行的具名检查（定义在工具 recipe 里）。
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    #[serde(default)]
    pub min: String,
    #[serde(default)]
    pub max: String,
    #[serde(default = "default_probe")]
    pub probe: Vec<String>,
    #[serde(default)]
    pub feature_checks: Vec<String>,
}

fn default_probe() -> Vec<String> {
    vec!["--version".to_owned()]
}

impl<'de> Deserialize<'de> for ToolSpec {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Name(String),
            Spec {
                name: String,
                #[serde(default)]
                min: String,
                #[serde(default)]
                max: String,
                #[serde(default = "default_probe")]
                probe: Vec<String>,
                #[serde(default)]
                feature_checks: Vec<String>,
            },
        }
        match Raw::deserialize(deserializer)? {
            Raw::Name(name) => Ok(Self {
                name,
                min: String::new(),
                max: String::new(),
                probe: default_probe(),
                feature_checks: Vec::new(),
            }),
            Raw::Spec {
                name,
                min,
                max,
                probe,
                feature_checks,
            } => Ok(Self {
                name,
                min,
                max,
                probe,
                feature_checks,
            }),
        }
    }
}

/// 产品的编译器版本档位。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolchain {
    #[serde(default)]
    pub gcc: Vec<ToolchainRange>,
    #[serde(default)]
    pub gxx: Vec<ToolchainRange>,
}

/// 一个编译器档位：版本区间 + 该档追加的编译参数。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolchainRange {
    #[serde(default)]
    pub min: String,
    #[serde(default)]
    pub max: String,
    #[serde(default)]
    pub cflags: String,
    #[serde(default)]
    pub cxxflags: String,
}

impl Toolchain {
    fn validate(&self, at: &str) -> Result<()> {
        if self.gcc.is_empty() && self.gxx.is_empty() {
            bail!("{at}: toolchain 至少要声明 gcc 或 gxx 中的一项");
        }
        validate_ranges(&self.gcc, "gcc", "cflags", at)?;
        validate_ranges(&self.gxx, "gxx", "cxxflags", at)?;
        Ok(())
    }
}

fn validate_ranges(
    ranges: &[ToolchainRange],
    item: &str,
    flag_field: &str,
    at: &str,
) -> Result<()> {
    let other = if flag_field == "cflags" {
        "cxxflags"
    } else {
        "cflags"
    };
    for range in ranges {
        for (field, value) in [("min", &range.min), ("max", &range.max)] {
            if !value.is_empty() && !is_version_text(value) {
                bail!("{at}: toolchain.{item} 的 {field}={value} 不是数字点分版本");
            }
        }
        if range.min.is_empty() && range.max.is_empty() {
            bail!("{at}: toolchain.{item} 的档位必须有 min 或 max");
        }
        let wrong = if flag_field == "cflags" {
            &range.cxxflags
        } else {
            &range.cflags
        };
        if !wrong.trim().is_empty() {
            bail!("{at}: toolchain.{item} 的档位只能写 {flag_field}，不能写 {other}");
        }
    }
    // 同一项里的档位不能重叠，保证“选了哪一档”是确定的。
    for (index, left) in ranges.iter().enumerate() {
        for right in &ranges[index + 1..] {
            if crate::version::ranges_overlap(
                (left.min.as_str(), left.max.as_str()),
                (right.min.as_str(), right.max.as_str()),
            ) {
                bail!(
                    "{at}: toolchain.{item} 的档位重叠：[{}, {}] 与 [{}, {}]",
                    display_bound(&left.min, "-∞"),
                    display_bound(&left.max, "+∞"),
                    display_bound(&right.min, "-∞"),
                    display_bound(&right.max, "+∞")
                );
            }
        }
    }
    Ok(())
}

fn display_bound(value: &str, empty: &str) -> String {
    if value.is_empty() {
        empty.to_owned()
    } else {
        value.to_owned()
    }
}

pub(crate) fn is_version_text(value: &str) -> bool {
    !value.is_empty()
        && value
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// 安装后动作；只允许固定原语，不执行任意 shell。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Action {
    EnsureDir {
        path: String,
    },
    WriteFile {
        path: String,
        content: String,
        #[serde(default)]
        if_missing: bool,
    },
    CopyCaBundle {
        to: String,
        candidates: Vec<String>,
    },
    WriteComponentIni {
        dir: String,
        register: String,
    },
}

impl Action {
    fn validate(&self) -> Result<()> {
        match self {
            Action::EnsureDir { path } => check_relative(path, "ensure-dir.path"),
            Action::WriteFile { path, .. } => check_relative(path, "write-file.path"),
            Action::CopyCaBundle { to, candidates } => {
                check_relative(to, "copy-ca-bundle.to")?;
                if candidates.is_empty() {
                    bail!("copy-ca-bundle.candidates 不能为空");
                }
                for candidate in candidates {
                    if candidate.is_empty() {
                        bail!("copy-ca-bundle.candidates 里有空路径");
                    }
                }
                Ok(())
            }
            Action::WriteComponentIni { dir, register } => {
                check_relative(dir, "write-component-ini.dir")?;
                if register.is_empty() {
                    bail!("write-component-ini.register 不能为空");
                }
                Ok(())
            }
        }
    }
}

pub(crate) fn check_relative(path: &str, field: &str) -> Result<()> {
    if path.is_empty() {
        bail!("{field} 不能为空");
    }
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        bail!("{field}={} 必须是安装树内的相对路径", path.display());
    }
    Ok(())
}

/// 编译器归 toolchain，永远不会从 Tool catalog 下载或构建。
pub(crate) fn is_compiler_name(name: &str) -> bool {
    matches!(name, "gcc" | "g++" | "gxx" | "cc" | "c++")
}

fn validate_tools(tools: &[ToolSpec], at: &str) -> Result<()> {
    for tool in tools {
        if tool.name.is_empty() {
            bail!("{at}: tools 里有空名字");
        }
        if is_compiler_name(&tool.name) {
            bail!(
                "{at}: tools 里不能写编译器 {}；编译器属于 toolchain，由产品 recipe 的 \
                 toolchain(...) 声明，只探测当前环境、不下载不安装",
                tool.name
            );
        }
        for (field, value) in [("min", &tool.min), ("max", &tool.max)] {
            if value.is_empty() {
                continue;
            }
            if tool.probe.is_empty() {
                bail!("{at}: tool {} 设置了 {field}，但 probe 为空", tool.name);
            }
            if !is_version_text(value) {
                bail!(
                    "{at}: tool {} 的 {field}={} 不是数字点分版本",
                    tool.name,
                    value
                );
            }
        }
        if !tool.min.is_empty()
            && !tool.max.is_empty()
            && crate::version::compare_to_bound(&tool.min, &tool.max) == std::cmp::Ordering::Greater
        {
            bail!(
                "{at}: tool {} 的 min={} 大于 max={}",
                tool.name,
                tool.min,
                tool.max
            );
        }
        for arg in &tool.probe {
            if arg.is_empty() {
                bail!("{at}: tool {} 的 probe 里有空参数", tool.name);
            }
        }
    }
    Ok(())
}

impl Profile {
    /// 某个登记动作写 ini 的目录，来自 post_install 的 write_component_ini。
    pub fn register_dir(&self, register: &str) -> Option<&str> {
        self.post_install.iter().find_map(|action| match action {
            Action::WriteComponentIni { dir, register: reg } if reg == register => {
                Some(dir.as_str())
            }
            _ => None,
        })
    }

    /// 组件 / 依赖不允许带任何产品 profile 内容。
    fn is_empty(&self) -> bool {
        self.modes.is_empty()
            && self.defaults.is_empty()
            && self.bundled_path.is_empty()
            && self.runtime.lib_dir.is_empty()
            && self.runtime.rpath.is_empty()
            && self.register.is_empty()
            && self.backup.is_none()
            && self.tools.is_empty()
            && self.post_install.is_empty()
            && self.verify.is_empty()
            && self.verify_modules.is_empty()
            && self.entry.is_empty()
            && self.entry_template.is_empty()
            && self.env.is_empty()
            && self.audit.is_empty()
    }

    fn validate(&self, at: &str) -> Result<()> {
        validate_tools(&self.tools, at)?;
        for action in &self.post_install {
            action.validate()?;
        }
        if !self.entry_template.is_empty() {
            let path = Path::new(&self.entry_template);
            if path.components().count() != 1
                || !matches!(
                    path.components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                bail!(
                    "{at}: entry_template={} 必须是产品目录下的文件名",
                    self.entry_template
                );
            }
        }
        if !self.entry.is_empty() && self.entry_template.is_empty() {
            bail!("{at}: 设置了 entry，但没有 entry_template");
        }
        Ok(())
    }

    pub fn strategy(&self, mode: &str) -> Result<Strategy> {
        let def = self
            .modes
            .get(mode)
            .ok_or_else(|| anyhow!("产品 profile 没有挂载方式 {mode}"))?;
        match def.strategy.as_str() {
            "fold" => Ok(Strategy::Fold),
            "standalone" => Ok(Strategy::Standalone),
            other => bail!("挂载方式 {mode} 的 strategy={other} 不合法，只能是 fold 或 standalone"),
        }
    }
}

/// 产品的一个构建目标（SAPI、可执行文件等）。
///
/// 只由产品 recipe 声明；选中状态由工具注入到 `Target::selected_targets`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildTarget {
    /// target 选中时应用的 configure 参数。
    #[serde(default)]
    pub flags: Vec<String>,
    /// target 未选中时应用的 configure 参数；不写就不生成。
    #[serde(default)]
    pub disable_flags: Vec<String>,
    /// target 选中时追加的产物。
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// target 选中时追加的安装后动作。
    #[serde(default)]
    pub post_install: Vec<Action>,
    /// target 选中时用于产品校验的命令。
    #[serde(default)]
    pub verify: Vec<String>,
    /// target 选中时用于模块校验的 shell 命令。
    #[serde(default)]
    pub verify_modules: String,
    /// target 选中时生成的入口名模板；空表示不生成。
    #[serde(default)]
    pub entry: String,
    /// 入口模板文件，相对产品 recipe 目录。
    #[serde(default)]
    pub entry_template: String,
    /// target 自己的配置项：名字 -> 类型和构建参数模板。
    #[serde(default)]
    pub config: BTreeMap<String, TargetConfig>,
}

impl BuildTarget {
    fn validate(&self, at: &str, name: &str) -> Result<()> {
        for (kind, list) in [
            ("flags", &self.flags),
            ("disable_flags", &self.disable_flags),
            ("artifacts", &self.artifacts),
        ] {
            let mut seen = BTreeSet::new();
            for item in list {
                if item.is_empty() {
                    bail!("{at}: target:{name} 的 {kind} 里有空值");
                }
                if !seen.insert(item) {
                    bail!("{at}: target:{name} 的 {kind} 重复: {item}");
                }
            }
        }
        for action in &self.post_install {
            action.validate()?;
        }
        if !self.entry_template.is_empty() {
            let path = Path::new(&self.entry_template);
            if path.components().count() != 1
                || !matches!(
                    path.components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                bail!(
                    "{at}: target:{name} 的 entry_template={} 必须是产品目录下的文件名",
                    self.entry_template
                );
            }
        }
        if !self.entry.is_empty() && self.entry_template.is_empty() {
            bail!("{at}: target:{name} 设置了 entry，但没有 entry_template");
        }
        for (key, config) in &self.config {
            if key.is_empty() {
                bail!("{at}: target:{name} 的 config 里有空名字");
            }
            if !is_target_name(key) {
                bail!("{at}: target:{name} 的配置项 {key} 含有不支持的字符");
            }
            match config.kind.as_str() {
                "string" | "list" => {}
                other => bail!(
                    "{at}: target:{name} 的配置项 {key} 的 type={other} 不合法（只支持 string / list）"
                ),
            }
            if config.flags.is_empty() {
                bail!("{at}: target:{name} 的配置项 {key} 缺少 flags");
            }
            if !config.flags.contains("{value}") {
                bail!(
                    "{at}: target:{name} 的配置项 {key} 的 flags 必须包含 {{value}}：{}",
                    config.flags
                );
            }
        }
        Ok(())
    }
}

/// target / 配置项名字只允许字母、数字、`-`、`_`。
fn is_target_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 一个 target 配置项的声明。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetConfig {
    /// `string` 或 `list`。
    #[serde(default, rename = "type")]
    pub kind: String,
    /// 含 `{value}` 的构建参数模板。
    #[serde(default)]
    pub flags: String,
}

/// target 配置值：字符串或字符串列表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TargetValue {
    One(String),
    Many(Vec<String>),
}

/// 规范化后的 target 选择：target 名/配置名按 BTreeMap 排序，列表保留元素顺序。
pub type TargetSelection = BTreeMap<String, BTreeMap<String, TargetValue>>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub kind: Kind,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub needs: Vec<String>,
    /// 构建期工具要求（dep / component / tool）；参与构建顺序和 PATH，不参与链接。
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    /// 产品：编译器版本档位；只做探测、选档和参数合并。
    #[serde(default)]
    pub toolchain: Option<Toolchain>,
    /// 产品：源码包内自带的组件名。
    #[serde(default)]
    pub bundled: Vec<String>,
    /// 产品：组件未选中时使用的关闭参数。
    #[serde(default)]
    pub bundled_disable: BTreeMap<String, String>,
    /// 产品：本次方案选中的组件名（由 state 或 CLI 注入）。
    #[serde(default)]
    pub components: Vec<String>,
    /// 产品：转给依赖库的 feature，如 `{ openssl = ["..."] }`。
    #[serde(default)]
    pub deps: BTreeMap<String, Vec<String>>,
    /// 产品 profile。
    #[serde(default)]
    pub profile: Profile,
    /// 产品：构建目标声明；target 名 -> 声明。
    #[serde(default)]
    pub targets: BTreeMap<String, BuildTarget>,
    /// 产品：本次选中的 target 及配置；由工具注入，recipe 不写。
    #[serde(skip)]
    pub selected_targets: TargetSelection,
    /// 组件：所属产品。
    #[serde(default)]
    pub product: String,
    /// 组件来源：`bundled` 或 `external`。
    #[serde(default)]
    pub source: String,
    /// 组件支持的挂载方式；第一个是默认值（也可以由 profile.defaults 覆盖）。
    #[serde(default)]
    pub modes: Vec<String>,
    /// 组件默认挂载方式，由产品 profile 的 defaults 注入。
    #[serde(default)]
    pub default_mode: String,
    /// bundled 组件的源码在产品源码树里的相对路径，如 `ext/openssl`。
    #[serde(default)]
    pub source_path: String,
    /// 产品启动后应看到的模块名；由产品 profile 的验证步骤使用。
    #[serde(default)]
    pub modules: Vec<String>,
    /// 组件安装后的登记文件；由产品 profile 的 register 动作使用。
    #[serde(default)]
    pub ini: String,
    #[serde(default)]
    pub ini_lines: Vec<String>,
    /// 组件自己的环境变量；fold 时合并进产品节点。
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 依赖集成：依赖名 -> 选中该依赖时额外加给组件的 configure 参数。
    /// 出现即表示允许 dep:NAME；key 必须在 needs 或某个 feature 的 needs 里。
    #[serde(default)]
    pub dep_configure: BTreeMap<String, Vec<String>>,
    /// 构建前在源码目录执行的命令（如给上游源码打兼容补丁）。
    #[serde(default)]
    pub prepare: Vec<String>,
    /// 具名检查：宿主命令存在且版本满足后再执行，见 `FeatureCheck`。
    #[serde(default)]
    pub feature_checks: BTreeMap<String, FeatureCheck>,
    #[serde(default)]
    pub features: BTreeMap<String, Feature>,
    /// 看过但决定不暴露的上游选项 -> 理由；只有 dep / component 使用。
    #[serde(default)]
    pub ignored: BTreeMap<String, String>,
    #[serde(default)]
    pub configure: Vec<String>,
    #[serde(default)]
    pub make: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub desc: String,
    /// 选中的版本线（6.2 / 3.5 / 8.5）。
    #[serde(default)]
    pub line: String,
    /// 产品 recipe 的系列（8.5）；其他目标为空。
    #[serde(default)]
    pub series: String,
}

impl Target {
    pub fn id(&self) -> String {
        match self.kind {
            Kind::Product => format!("product:{}@{}", self.name, self.version),
            Kind::Tool => format!("tool:{}@{}", self.name, self.version),
            _ => format!("{}:{}", self.kind.as_str(), self.name),
        }
    }

    fn validate(&self, path: &Path) -> Result<()> {
        let at = path.display();
        if self.name.is_empty() {
            bail!("{at}: name 不能为空");
        }
        match self.kind {
            Kind::Dep => {
                if self.version.is_empty() {
                    bail!("{at}: dep:{} 缺少 version", self.name);
                }
                if self.url.is_empty() {
                    bail!("{at}: dep:{} 缺少 url", self.name);
                }
                self.reject_product_fields(&at)?;
                validate_tools(&self.tools, &at.to_string())?;
                if !self.dep_configure.is_empty() {
                    bail!("{at}: dep:{} 不允许 dep_configure", self.name);
                }
                if !self.prepare.is_empty() {
                    bail!("{at}: dep:{} 不允许 prepare", self.name);
                }
                for (name, f) in &self.features {
                    if !f.requires.product.is_empty() || !f.requires.dep.is_empty() {
                        bail!(
                            "{at}: dep:{} 的 feature {name} 只允许 requires.commands",
                            self.name
                        );
                    }
                    for command in &f.requires.commands {
                        if command.is_empty() {
                            bail!(
                                "{at}: dep:{} 的 feature {name} 的 requires.commands 里有空名字",
                                self.name
                            );
                        }
                    }
                }
                self.check_ignored(&at)?;
            }
            Kind::Component => {
                if self.product.is_empty() {
                    bail!("{at}: component:{} 缺少 product", self.name);
                }
                if self.source != "bundled" && self.source != "external" {
                    bail!(
                        "{at}: component:{} 的 source={} 不合法，只能是 bundled 或 external",
                        self.name,
                        self.source
                    );
                }
                if self.source == "external" && (self.url.is_empty() || self.version.is_empty()) {
                    bail!(
                        "{at}: component:{} 是 external，但缺少 url 或 version",
                        self.name
                    );
                }
                if self.modes.is_empty() {
                    bail!("{at}: component:{} 缺少 modes", self.name);
                }
                for mode in &self.modes {
                    if mode.is_empty() {
                        bail!("{at}: component:{} 的 modes 里有空名字", self.name);
                    }
                }
                if !self.modes.iter().any(|m| m == &self.default_mode) {
                    bail!(
                        "{at}: component:{} 的默认挂载方式 {} 不在 modes 里（{}）",
                        self.name,
                        self.default_mode,
                        self.modes.join(", ")
                    );
                }
                if !self.bundled.is_empty()
                    || !self.bundled_disable.is_empty()
                    || !self.profile.is_empty()
                    || !self.components.is_empty()
                    || !self.series.is_empty()
                    || !self.targets.is_empty()
                {
                    bail!("{at}: component:{} 不允许产品字段", self.name);
                }
                validate_tools(&self.tools, &at.to_string())?;
                for (dep, flags) in &self.dep_configure {
                    if dep.is_empty() {
                        bail!("{at}: component:{} 的 dep_configure 里有空名字", self.name);
                    }
                    let in_feature = self
                        .features
                        .values()
                        .any(|f| f.needs.iter().any(|n| n == dep));
                    if !self.needs.iter().any(|n| n == dep) && !in_feature {
                        bail!(
                            "{at}: component:{} 的 dep_configure 里有 dep:{dep}，\
                             但它不在 needs 或任何 feature 的 needs 里",
                            self.name
                        );
                    }
                    for flag in flags {
                        if flag.is_empty() {
                            bail!(
                                "{at}: component:{} 的 dep_configure[{dep}] 里有空参数",
                                self.name
                            );
                        }
                    }
                }
                for (name, f) in &self.features {
                    if name.is_empty() {
                        bail!("{at}: component:{} 的 features 里有空名字", self.name);
                    }
                    if !f.disable.is_empty() {
                        bail!(
                            "{at}: component:{} 的 feature {name} 只允许 needs/configure/requires；\
                             disable 只用于 dep 的 feature",
                            self.name
                        );
                    }
                    for need in &f.needs {
                        if need.is_empty() {
                            bail!(
                                "{at}: component:{} 的 feature {name} 的 needs 里有空名字",
                                self.name
                            );
                        }
                    }
                    for flag in &f.configure {
                        if flag.is_empty() {
                            bail!("{at}: component:{} 的 feature {name} 里有空参数", self.name);
                        }
                    }
                    for series in &f.requires.product {
                        if series.is_empty() {
                            bail!(
                                "{at}: component:{} 的 feature {name} 的 requires.product 里有空名字",
                                self.name
                            );
                        }
                    }
                    for (dep, lines) in &f.requires.dep {
                        if !self.needs.iter().any(|n| n == dep) {
                            bail!(
                                "{at}: component:{} 的 feature {name} 的 requires.dep 里有 {dep}，\
                                 但它不在 needs 里",
                                self.name
                            );
                        }
                        if lines.is_empty() {
                            bail!(
                                "{at}: component:{} 的 feature {name} 的 requires.dep[{dep}] 是空的",
                                self.name
                            );
                        }
                        for line in lines {
                            if line.is_empty() {
                                bail!(
                                    "{at}: component:{} 的 feature {name} 的 requires.dep[{dep}] 里有空线",
                                    self.name
                                );
                            }
                        }
                    }
                    for command in &f.requires.commands {
                        if command.is_empty() {
                            bail!(
                                "{at}: component:{} 的 feature {name} 的 requires.commands 里有空名字",
                                self.name
                            );
                        }
                    }
                }
                for key in self.env.keys() {
                    if key.is_empty() {
                        bail!("{at}: component:{} 的 env 里有空名字", self.name);
                    }
                }
            }
            Kind::Tool => {
                if is_compiler_name(&self.name) {
                    bail!(
                        "{at}: 不允许 tool:{}；编译器属于 toolchain，不能从 Tool catalog 构建",
                        self.name
                    );
                }
                if self.version.is_empty() {
                    bail!("{at}: tool:{} 缺少 version", self.name);
                }
                if self.url.is_empty() {
                    bail!("{at}: tool:{} 缺少 url", self.name);
                }
                self.reject_product_fields(&at)?;
                if !self.needs.is_empty() {
                    bail!(
                        "{at}: tool:{} 不允许 needs（工具不能依赖库，只能依赖工具）",
                        self.name
                    );
                }
                if !self.deps.is_empty()
                    || !self.components.is_empty()
                    || !self.modes.is_empty()
                    || !self.source.is_empty()
                    || !self.series.is_empty()
                {
                    bail!("{at}: tool:{} 不允许产品 / 组件字段", self.name);
                }
                if !self.prepare.is_empty()
                    || !self.features.is_empty()
                    || !self.dep_configure.is_empty()
                    || !self.ignored.is_empty()
                {
                    bail!(
                        "{at}: tool:{} 只允许 tools / configure / make / artifacts 这些构建字段",
                        self.name
                    );
                }
                validate_tools(&self.tools, &at.to_string())?;
            }
            Kind::Product => {
                if !self.prepare.is_empty() {
                    bail!("{at}: product:{} 不允许 prepare", self.name);
                }
                if self.series.is_empty() {
                    bail!("{at}: product:{} 缺少 series", self.name);
                }
                if !self.dep_configure.is_empty()
                    || !self.features.is_empty()
                    || !self.ignored.is_empty()
                {
                    bail!(
                        "{at}: product:{} 不允许 dep_configure/features/ignored",
                        self.name
                    );
                }
                for (dep, feats) in &self.deps {
                    if dep.is_empty() || feats.iter().any(|f| f.is_empty()) {
                        bail!("{at}: product:{} 的 deps 里有空名字", self.name);
                    }
                }
                if self.version.is_empty() {
                    bail!("{at}: product:{} 缺少 version", self.name);
                }
                if self.url.is_empty() {
                    bail!("{at}: product:{} 缺少 url", self.name);
                }
                if let Some(toolchain) = &self.toolchain {
                    toolchain.validate(&at.to_string())?;
                }
                self.profile.validate(&at.to_string())?;
                if self.profile.modes.is_empty() {
                    bail!("{at}: product:{} 的 profile.modes 不能为空", self.name);
                }
                for mode in self.profile.modes.keys() {
                    if mode.is_empty() {
                        bail!("{at}: product:{} 的 profile.modes 里有空名字", self.name);
                    }
                }
                for (source, mode) in &self.profile.defaults {
                    if source != "bundled" && source != "external" {
                        bail!(
                            "{at}: product:{} 的 profile.defaults 键 {source} 不合法，\
                             只能是 bundled 或 external",
                            self.name
                        );
                    }
                    if !self.profile.modes.contains_key(mode) {
                        bail!(
                            "{at}: product:{} 的 profile.defaults.{source}={mode} 不在 profile.modes 里",
                            self.name
                        );
                    }
                }
                if let Some(backup) = &self.profile.backup {
                    backup.validate(&at.to_string())?;
                }
                for mode in self.profile.register.keys() {
                    if !self.profile.modes.contains_key(mode) {
                        bail!(
                            "{at}: product:{} 的 profile.register.{mode} 不在 profile.modes 里",
                            self.name
                        );
                    }
                }
                match self.profile.audit.as_str() {
                    "" | "none" | "php-config-m4" | "configure-help" => {}
                    other => bail!(
                        "{at}: product:{} 的 profile.audit={other} 不合法\
                         （只支持 php-config-m4 / configure-help / none）",
                        self.name
                    ),
                }
                let mut seen = BTreeSet::new();
                for name in &self.bundled {
                    if name.is_empty() {
                        bail!("{at}: product:{} 的 bundled 里有空名字", self.name);
                    }
                    if !seen.insert(name.clone()) {
                        bail!("{at}: product:{} 的 bundled 重复: {name}", self.name);
                    }
                }
                for (name, flag) in &self.bundled_disable {
                    if !seen.contains(name) {
                        bail!(
                            "{at}: product:{} 的 bundled_disable 里有非内置组件 {name}",
                            self.name
                        );
                    }
                    if flag.contains(char::is_whitespace)
                        || !(flag.starts_with("--without-") || flag.starts_with("--disable-"))
                    {
                        bail!(
                            "{at}: product:{} 的 bundled_disable[{name}] 必须是单个 \
                             --without-* 或 --disable-* 参数",
                            self.name
                        );
                    }
                }
                for (key, value) in &self.profile.env {
                    if key.is_empty() {
                        bail!("{at}: product:{} 的 profile.env 里有空名字", self.name);
                    }
                    if value.is_empty() {
                        bail!("{at}: product:{} 的 profile.env[{key}] 是空的", self.name);
                    }
                }
                for (name, target) in &self.targets {
                    if !is_target_name(name) {
                        bail!(
                            "{at}: product:{} 的 target 名 {name} 只允许字母、数字、- 和 _",
                            self.name
                        );
                    }
                    target.validate(&at.to_string(), name)?;
                }
            }
        }
        self.check_ignored(&at)?;
        Ok(())
    }

    fn reject_product_fields(&self, at: &std::path::Display<'_>) -> Result<()> {
        let kind = self.kind.as_str();
        if !self.bundled.is_empty() || !self.bundled_disable.is_empty() {
            bail!("{at}: {kind}:{} 不允许 bundled/bundled_disable", self.name);
        }
        if !self.components.is_empty() || !self.profile.is_empty() {
            bail!("{at}: {kind}:{} 不允许产品字段", self.name);
        }
        if !self.modes.is_empty() || !self.source.is_empty() || !self.product.is_empty() {
            bail!("{at}: {kind}:{} 不允许组件字段", self.name);
        }
        if !self.targets.is_empty() {
            bail!("{at}: {kind}:{} 不允许 targets", self.name);
        }
        if self.toolchain.is_some() {
            bail!(
                "{at}: {kind}:{} 不允许 toolchain（只写在产品 recipe）",
                self.name
            );
        }
        Ok(())
    }

    fn check_ignored(&self, at: &std::path::Display<'_>) -> Result<()> {
        for (option, reason) in &self.ignored {
            if !option.starts_with("--") {
                bail!(
                    "{at}: {}:{} 的 ignored key {option} 必须是 --enable-* / --with-* 形式的选项",
                    self.kind.as_str(),
                    self.name
                );
            }
            if reason.trim().is_empty() {
                bail!(
                    "{at}: {}:{} 的 ignored[{option}] 需要写理由",
                    self.kind.as_str(),
                    self.name
                );
            }
        }
        Ok(())
    }
}

/// recipe 声明的一条版本线：支持哪些产品系列，以及依赖哪些线。
#[derive(Debug, Clone, Default)]
pub struct LineMeta {
    pub product: Vec<String>,
    pub deps: BTreeMap<String, Vec<String>>,
}

/// 第一阶段加载出来的 recipe 元数据，不需要构建上下文。
#[derive(Debug, Clone)]
pub struct RecipeMeta {
    pub kind: Kind,
    pub name: String,
    /// 产品的系列；组件为空。
    pub series: Option<String>,
    /// 组件所属的产品；产品 / 依赖库为空。
    pub product: Option<String>,
    pub lines: BTreeMap<String, LineMeta>,
    /// 产品系列 -> 该组件允许的依赖线（内置组件用，如 component:openssl）。
    pub product_lines: BTreeMap<String, Vec<String>>,
    /// 产品系列 -> 依赖名 -> 允许的依赖线；用于没有同名依赖的内置组件（如 gd）。
    pub product_dep_lines: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// 产品 recipe 的 bundled 组件事实；其他 recipe 为空。
    pub bundled: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ComponentSpec {
    pub mode: Option<String>,
    /// 依赖名 -> 转给该依赖的 feature 列表
    pub deps: BTreeMap<String, Vec<String>>,
    /// `feature:[...]` 选中的组件自己的 feature；None 表示没写，
    /// Some(空) 表示显式关闭（用于关掉 bundle 默认值）。
    pub features: Option<BTreeSet<String>>,
    /// `version:6.2` 指定的版本线；只在这里记录，绑定在加载 recipe 前完成。
    pub version: Option<String>,
}

/// 命令行 `--component` 里与版本线有关的最小信息，用于加载 recipe 之前解析绑定。
#[derive(Debug, Clone, Default)]
pub struct RawComponent {
    pub name: String,
    pub version: Option<String>,
}

/// 只解析组件 spec 的名字和 `version:`，不依赖 recipe。
pub fn preparse(specs: &[String]) -> Result<Vec<RawComponent>> {
    preparse_with(specs, false)
}

/// 方案列表的预解析：一个 spec 里允许多个组件。
pub fn preparse_scheme(list: &str) -> Result<Vec<RawComponent>> {
    preparse_with(&[list.to_owned()], true)
}

fn preparse_with(specs: &[String], multiple: bool) -> Result<Vec<RawComponent>> {
    let mut out: Vec<RawComponent> = Vec::new();
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();

    for spec in specs {
        let mut current: Option<usize> = None;
        for raw in split_top(spec, ',')? {
            let part = raw.trim();
            if part.is_empty() {
                bail!("组件 spec {spec} 里有空的片段");
            }
            if part.starts_with("mode:")
                || part.starts_with("dep:")
                || part.starts_with("feature:")
                || part.starts_with("version:")
            {
                let Some(idx) = current else {
                    bail!("组件 spec {spec} 的第一个片段不能是 {part}");
                };
                if let Some(version) = part.strip_prefix("version:") {
                    if version.is_empty() {
                        bail!("组件 spec {spec} 的 version: 后面缺少版本线");
                    }
                    if out[idx].version.replace(version.to_owned()).is_some() {
                        bail!("组件 spec 里重复指定了 {} 的版本线", out[idx].name);
                    }
                }
                continue;
            }
            if current.is_some() && !multiple {
                bail!(
                    "一次只能指定一个组件：{spec} 里的 {part} 是第二个组件名；\
                     多个组件请分多条命令"
                );
            }

            let (name, value) = match part.find(['=', ':']) {
                Some(i) => (&part[..i], Some(&part[i + 1..])),
                None => (part, None),
            };
            if name.is_empty() {
                bail!("组件 spec {part} 缺少组件名");
            }
            if seen.insert(name.to_owned(), ()).is_some() {
                bail!("组件 spec 里重复指定了组件 {name}");
            }
            let mut component = RawComponent {
                name: name.to_owned(),
                version: None,
            };
            if let Some(value) = value {
                if value.is_empty() {
                    bail!("组件 {name} 的选项是空的");
                }
                if let Some(version) = value.strip_prefix("version:") {
                    if version.is_empty() {
                        bail!("组件 {name} 的 version: 后面缺少版本线");
                    }
                    component.version = Some(version.to_owned());
                }
            }
            current = Some(out.len());
            out.push(component);
        }
    }
    Ok(out)
}

/// 命令行 `--component` 的选择结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Selection {
    pub per_component: BTreeMap<String, ComponentSpec>,
}

/// 解析一份完整方案列表：逗号分隔多个组件，选项并入前一个组件。
/// 与 `--component` 的唯一区别是这里允许一次写多个组件。
pub fn parse_scheme(reg: &Registry, list: &str) -> Result<Selection> {
    if list.trim().is_empty() {
        return Ok(Selection::default());
    }
    let mut specs: Vec<String> = Vec::new();
    for raw in split_top(list, ',')? {
        let part = raw.trim();
        if part.is_empty() {
            bail!("方案列表 {list} 里有空的片段");
        }
        let is_option = part.starts_with("mode:")
            || part.starts_with("dep:")
            || part.starts_with("feature:")
            || part.starts_with("version:");
        match (specs.last_mut(), is_option) {
            (Some(last), true) => {
                last.push(',');
                last.push_str(part);
            }
            (None, true) => bail!("方案列表 {list} 的第一个片段不能是 {part}"),
            _ => specs.push(part.to_owned()),
        }
    }
    Selection::parse(reg, &specs)
}

impl Selection {
    pub fn parse(reg: &Registry, specs: &[String]) -> Result<Self> {
        let mut sel = Selection::default();

        for spec in specs {
            // 逗号既分隔组件，也分隔同一个组件的选项；[] 里的逗号不参与切分。
            let mut chunks: Vec<String> = Vec::new();
            for raw in split_top(spec, ',')? {
                let part = raw.trim();
                if part.is_empty() {
                    bail!("组件 spec {spec} 里有空的片段");
                }
                if part.starts_with("mode:")
                    || part.starts_with("dep:")
                    || part.starts_with("feature:")
                    || part.starts_with("version:")
                {
                    let Some(last) = chunks.last_mut() else {
                        bail!("组件 spec {spec} 的第一个片段不能是 {part}");
                    };
                    last.push(',');
                    last.push_str(part);
                } else {
                    if !chunks.is_empty() {
                        bail!(
                            "一次只能指定一个组件：{spec} 里的 {part} 是第二个组件名；\
                             多个组件请分多条命令"
                        );
                    }
                    chunks.push(part.to_owned());
                }
            }

            for chunk in chunks {
                let (name, value) = match chunk.find(['=', ':']) {
                    Some(i) => (&chunk[..i], Some(&chunk[i + 1..])),
                    None => (chunk.as_str(), None),
                };
                if name.is_empty() {
                    bail!("组件 spec {chunk} 缺少组件名");
                }
                let component = reg.component(name)?;
                if sel.per_component.contains_key(name) {
                    bail!("组件 spec 里重复指定了组件 {name}");
                }

                let mut spec = ComponentSpec::default();
                if let Some(value) = value {
                    if value.is_empty() {
                        bail!("组件 {name} 的选项是空的");
                    }
                    for item in split_top(value, ',')? {
                        let item = item.trim();
                        if let Some(m) = item.strip_prefix("mode:") {
                            if !component.modes.iter().any(|mode| mode == m) {
                                bail!(
                                    "component:{name} 契约不支持 mode:{m}（支持：{}）",
                                    component.modes.join(",")
                                );
                            }
                            reg.product_mode(component, m)?;
                            spec.mode = Some(m.to_owned());
                        } else if let Some(rest) = item.strip_prefix("dep:") {
                            let (dep, features) = parse_dep(reg, component, rest)?;
                            if spec.deps.insert(dep.clone(), features).is_some() {
                                bail!("组件 {name} 里重复指定了 dep:{dep}");
                            }
                        } else if let Some(rest) = item.strip_prefix("feature:") {
                            let feats = parse_feature(reg, component, rest)?;
                            let set = spec.features.get_or_insert_with(BTreeSet::new);
                            for f in feats {
                                set.insert(f);
                            }
                        } else if let Some(version) = item.strip_prefix("version:") {
                            if version.is_empty() {
                                bail!("组件 {name} 的 version: 后面缺少版本线");
                            }
                            if spec.version.replace(version.to_owned()).is_some() {
                                bail!("组件 {name} 里重复指定了 version:");
                            }
                        } else {
                            bail!(
                                "组件 {name} 里未知选项 {item}\
                                 （只支持 mode: / dep: / feature: / version:）"
                            );
                        }
                    }
                }

                sel.per_component.insert(name.to_owned(), spec);
            }
        }
        Ok(sel)
    }

    pub fn mode_of(&self, component: &Target) -> String {
        self.per_component
            .get(&component.name)
            .and_then(|s| s.mode.clone())
            .unwrap_or_else(|| component.default_mode.clone())
    }

    pub fn deps_of<'a>(
        &'a self,
        component: &Target,
    ) -> impl Iterator<Item = (&'a String, &'a Vec<String>)> + 'a {
        self.per_component
            .get(&component.name)
            .map(|s| s.deps.iter())
            .into_iter()
            .flatten()
    }

    /// 选中的组件 feature；None 表示没写，Some(空) 表示显式关闭。
    pub fn features_of(&self, component: &Target) -> Option<&BTreeSet<String>> {
        self.per_component
            .get(&component.name)
            .and_then(|s| s.features.as_ref())
    }

    /// 选中的组件 feature 额外需要的依赖；没选中就返回空。
    pub fn feature_needs<'a>(&'a self, component: &'a Target) -> Vec<&'a String> {
        let Some(selected) = self.features_of(component) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for name in selected {
            if let Some(def) = component.features.get(name) {
                out.extend(def.needs.iter());
            }
        }
        out
    }

    /// 本次选择给各 dep 带来的 feature 并集。
    pub fn features(&self, reg: &Registry) -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for component in reg.of_kind(Kind::Component) {
            for (dep, feats) in self.deps_of(component) {
                if !component.dep_configure.contains_key(dep) || feats.is_empty() {
                    continue;
                }
                let entry = out.entry(dep.clone()).or_default();
                for f in feats {
                    if !entry.contains(f) {
                        entry.push(f.clone());
                    }
                }
            }
        }
        for feats in out.values_mut() {
            feats.sort();
        }
        out
    }
}

/// `dep:NAME` 或 `dep:NAME[f1,f2]`；名字是依赖名，方括号里的 feature 转给该依赖。
fn parse_dep(reg: &Registry, component: &Target, rest: &str) -> Result<(String, Vec<String>)> {
    let (name, list) = match rest.find('[') {
        Some(i) => {
            if !rest.ends_with(']') {
                bail!(
                    "component:{} 的 dep: 写法不完整：dep:{rest}",
                    component.name
                );
            }
            (&rest[..i], Some(&rest[i + 1..rest.len() - 1]))
        }
        None => (rest, None),
    };
    if name.is_empty() {
        bail!("component:{} 的 dep: 后面缺少依赖名", component.name);
    }
    if !component.dep_configure.contains_key(name) {
        let available: Vec<&str> = component.dep_configure.keys().map(String::as_str).collect();
        let hint = if available.is_empty() {
            "无".to_owned()
        } else {
            available.join(", ")
        };
        bail!(
            "component:{} 没有依赖集成 {name}（可写：{hint}）；\
             需要在 recipe 的 dep_configure 里声明",
            component.name
        );
    }
    let dep = reg.get(&format!("dep:{name}")).ok_or_else(|| {
        anyhow!(
            "component:{} 的 dep:{name} 指向不存在的依赖",
            component.name
        )
    })?;

    let mut features = Vec::new();
    if let Some(list) = list {
        for f in list.split([',', '+']) {
            let f = f.trim();
            if f.is_empty() {
                bail!(
                    "component:{} 的 dep:{name} 的 feature 列表里有空项",
                    component.name
                );
            }
            if !dep.features.contains_key(f) {
                bail!("dep:{} 契约没有 feature {f}", dep.name);
            }
            feature_available(reg, dep, f)?;
            if !features.iter().any(|x| x == f) {
                features.push(f.to_owned());
            }
        }
    }
    Ok((name.to_owned(), features))
}

/// `feature:[f1,f2]`；必须带方括号，空列表表示显式关闭全部 feature。
fn parse_feature(reg: &Registry, component: &Target, rest: &str) -> Result<Vec<String>> {
    let list = match rest.strip_prefix('[') {
        Some(inner) => inner.strip_suffix(']').ok_or_else(|| {
            anyhow!(
                "component:{} 的 feature: 写法不完整：feature:{rest}",
                component.name
            )
        })?,
        None => bail!(
            "component:{} 的 feature: 后面必须写方括号，例如 feature:[argon2]",
            component.name
        ),
    };

    let mut features = Vec::new();
    if list.trim().is_empty() {
        return Ok(features);
    }
    for f in list.split(',') {
        let f = f.trim();
        if f.is_empty() {
            bail!("component:{} 的 feature 列表里有空项", component.name);
        }
        if !component.features.contains_key(f) {
            let available: Vec<&str> = component.features.keys().map(String::as_str).collect();
            let hint = if available.is_empty() {
                "无".to_owned()
            } else {
                available.join(", ")
            };
            bail!(
                "component:{} 没有 feature {f}（可用：{hint}）",
                component.name
            );
        }
        feature_available(reg, component, f)?;
        if !features.iter().any(|x| x == f) {
            features.push(f.to_owned());
        }
    }
    Ok(features)
}

/// 检查一个 feature 在当前产品系列和依赖线下是否可用。
pub(crate) fn feature_available(reg: &Registry, target: &Target, name: &str) -> Result<()> {
    let Some(def) = target.features.get(name) else {
        return Ok(());
    };
    let req = &def.requires;
    if req.product.is_empty() && req.dep.is_empty() && req.commands.is_empty() {
        return Ok(());
    }

    if !req.product.is_empty() {
        let product = reg.product_any()?;
        if !req.product.iter().any(|s| s == &product.series) {
            bail!(
                "{} 的 feature {name} 需要产品系列 {}，当前 {}",
                target.id(),
                req.product.join(", "),
                product.series
            );
        }
    }
    for (dep_name, lines) in &req.dep {
        let dep = reg.get(&format!("dep:{dep_name}")).ok_or_else(|| {
            anyhow!(
                "{} 的 feature {name} 需要不存在的 dep:{dep_name}",
                target.id()
            )
        })?;
        if !lines.iter().any(|l| l == &dep.line) {
            let current = if dep.line.is_empty() {
                "未绑定".to_owned()
            } else {
                dep.line.clone()
            };
            bail!(
                "{} 的 feature {name} 需要 dep:{dep_name} {}，当前 {current}",
                target.id(),
                lines.join(", ")
            );
        }
    }
    for command in &req.commands {
        if !crate::tools::on_path(command) {
            bail!(
                "{}",
                crate::host_commands::feature_hint(&reg.host, &target.id(), name, command)
            );
        }
    }
    Ok(())
}

pub(crate) fn split_top(s: &str, sep: char) -> Result<Vec<&str>> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                if depth == 0 {
                    bail!("{s} 里有多余的 ]");
                }
                depth -= 1;
            }
            _ if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        bail!("{s} 里缺少 ]");
    }
    parts.push(&s[start..]);
    Ok(parts)
}

#[derive(Debug, Default)]
pub struct Registry {
    pub targets: BTreeMap<String, Target>,
    /// 宿主环境：feature 的 `requires.commands` 用它生成安装提示。
    pub host: crate::host_commands::Env,
    /// 工具名 -> 具名检查；来自 `recipes/tools/<名字>.star`，含只能由系统提供的工具。
    pub host_tools: BTreeMap<String, BTreeMap<String, FeatureCheck>>,
}

impl Registry {
    pub fn from_targets(targets: impl IntoIterator<Item = Target>) -> Result<Self> {
        let mut map = BTreeMap::new();
        for t in targets {
            let id = t.id();
            if map.insert(id.clone(), t).is_some() {
                bail!("重复的目标 {id}");
            }
        }
        let reg = Self {
            targets: map,
            host: crate::host_commands::Env::default(),
            host_tools: BTreeMap::new(),
        };
        reg.validate_refs()?;
        Ok(reg)
    }

    pub fn get(&self, id: &str) -> Option<&Target> {
        self.targets.get(id)
    }

    pub fn of_kind(&self, kind: Kind) -> impl Iterator<Item = &Target> {
        self.targets.values().filter(move |t| t.kind == kind)
    }

    /// 本次加载的产品；每次只允许一个。
    pub fn product_any(&self) -> Result<&Target> {
        let mut it = self.of_kind(Kind::Product);
        let first = it
            .next()
            .ok_or_else(|| anyhow!("recipes 里没有 product recipe"))?;
        if it.next().is_some() {
            bail!("当前一次只能加载一个产品");
        }
        Ok(first)
    }

    pub fn product(&self, name: &str) -> Result<&Target> {
        let product = self.product_any()?;
        if product.name != name {
            bail!("当前加载的产品是 {}，不是 {name}", product.name);
        }
        Ok(product)
    }

    pub fn component(&self, name: &str) -> Result<&Target> {
        self.get(&format!("component:{name}"))
            .ok_or_else(|| anyhow!("未知组件 component:{name}"))
    }

    /// 组件默认挂载方式，来自产品 profile 的 defaults。
    pub fn default_mode(&self, component: &Target) -> Result<String> {
        let product = self.product(&component.product)?;
        let mode = product
            .profile
            .defaults
            .get(&component.source)
            .ok_or_else(|| {
                anyhow!(
                    "产品 {} 的 profile.defaults 没有声明 {} 组件的默认挂载方式",
                    product.name,
                    component.source
                )
            })?;
        Ok(mode.clone())
    }

    /// 校验并返回产品 profile 里的挂载方式定义。
    pub fn product_mode<'a>(&'a self, component: &Target, mode: &str) -> Result<&'a ProfileMode> {
        let product = self.product(&component.product)?;
        product
            .profile
            .modes
            .get(mode)
            .ok_or_else(|| anyhow!("产品 {} 的 profile 没有挂载方式 {mode}", product.name))
    }

    pub fn resolve_name(&self, name: &str) -> Result<String> {
        if self.targets.contains_key(name) {
            return Ok(name.to_owned());
        }
        let matches: Vec<&String> = self
            .targets
            .keys()
            .filter(|id| {
                let Some((_, suffix)) = id.split_once(':') else {
                    return false;
                };
                suffix == name || suffix.split('@').next() == Some(name)
            })
            .collect();
        match matches.as_slice() {
            [] => bail!("未知目标 {name}"),
            [one] => Ok((*one).clone()),
            many => bail!(
                "目标名 {name} 有歧义，请写成 {}",
                many.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" 或 ")
            ),
        }
    }

    fn validate_refs(&self) -> Result<()> {
        for t in self.targets.values() {
            for need in &t.needs {
                let id = format!("dep:{need}");
                if !self.targets.contains_key(&id) {
                    bail!("{} 的 needs 指向不存在的依赖 {id}", t.id());
                }
            }
            for tool in &t.tools {
                if tool.name.is_empty() {
                    bail!("{} 的 tools 里有空名字", t.id());
                }
            }
            for (name, f) in &t.features {
                for need in &f.needs {
                    let id = format!("dep:{need}");
                    if !self.targets.contains_key(&id) {
                        bail!("{} 的 feature {name} 指向不存在的依赖 {id}", t.id());
                    }
                }
            }
            for dep in t.dep_configure.keys() {
                let id = format!("dep:{dep}");
                if !self.targets.contains_key(&id) {
                    bail!("{} 的 dep_configure 指向不存在的依赖 {id}", t.id());
                }
            }
            match t.kind {
                Kind::Product => {
                    for name in &t.components {
                        let id = format!("component:{name}");
                        let Some(component) = self.targets.get(&id) else {
                            bail!("{} 的 components 指向不存在的组件 {id}", t.id());
                        };
                        if component.product != t.name {
                            bail!("{id} 不属于产品 {}", t.name);
                        }
                    }
                    for name in &t.bundled {
                        let id = format!("component:{name}");
                        if let Some(component) = self.targets.get(&id)
                            && component.source != "bundled"
                        {
                            bail!("{} 的 bundled 里 {name} 不是 bundled 组件", t.id());
                        }
                    }
                    for mode in t.profile.modes.keys() {
                        t.profile.strategy(mode)?;
                    }
                }
                Kind::Component => {
                    let product = self.product(&t.product)?;
                    for mode in &t.modes {
                        self.product_mode(t, mode)?;
                    }
                    if !t.default_mode.is_empty() {
                        self.product_mode(t, &t.default_mode)?;
                        if !t.modes.iter().any(|m| m == &t.default_mode) {
                            bail!("{} 的默认挂载方式 {} 不在 modes 里", t.id(), t.default_mode);
                        }
                    }
                    if t.source == "bundled" && !product.bundled.contains(&t.name) {
                        bail!(
                            "{} 是 bundled，但产品 {} 的 bundled 列表里没有它",
                            t.id(),
                            product.name
                        );
                    }
                }
                Kind::Dep => {}
                Kind::Tool => {
                    if t.needs.iter().any(|need| !need.is_empty()) {
                        bail!("{} 不允许 needs", t.id());
                    }
                }
            }
        }
        Ok(())
    }
}

/// 第一阶段：只读 recipe 顶层数据，建立版本关系，不需要 ctx。
pub fn load_meta(dir: &Path, catalog: &Catalog) -> Result<Vec<RecipeMeta>> {
    let defs = read_defs(dir)?;
    let globals = Globals::standard();
    let dialect = Dialect::Standard;
    let mut metas = Vec::new();

    for file in recipe_files(dir)? {
        let src = fs::read_to_string(&file.path)
            .with_context(|| format!("读不到 {}", file.path.display()))?;
        let prelude = read_prelude(&file.path)?;
        let raw = eval_meta(
            &file.path,
            &defs,
            prelude.as_deref(),
            &src,
            &globals,
            &dialect,
        )?;
        let lines = match raw.lines_json {
            Some(value) => parse_lines(value)
                .with_context(|| format!("{}: LINES 不合法", file.path.display()))?,
            None => BTreeMap::new(),
        };
        let product_lines = match raw.product_lines_json {
            Some(value) => parse_product_lines(value)
                .with_context(|| format!("{}: PRODUCT_LINES 不合法", file.path.display()))?,
            None => BTreeMap::new(),
        };
        let product_dep_lines = match raw.product_dep_lines_json {
            Some(value) => parse_product_dep_lines(value)
                .with_context(|| format!("{}: PRODUCT_DEP_LINES 不合法", file.path.display()))?,
            None => BTreeMap::new(),
        };
        if matches!(file.kind, Kind::Dep | Kind::Tool)
            && (!lines.is_empty() || !product_lines.is_empty())
        {
            bail!(
                "{}: {} recipe 不能声明 LINES/PRODUCT_LINES",
                file.path.display(),
                file.kind.as_str()
            );
        }
        if file.kind != Kind::Component && !product_dep_lines.is_empty() {
            bail!(
                "{}: PRODUCT_DEP_LINES 只用于 component recipe",
                file.path.display()
            );
        }
        if file.kind == Kind::Component {
            for (line, meta) in &lines {
                if meta.product.is_empty() {
                    bail!(
                        "{}: LINES.{line} 缺少 product 数组（组件必须声明支持哪些产品系列）",
                        file.path.display()
                    );
                }
            }
        }

        let mut bundled = Vec::new();
        if file.kind == Kind::Product {
            if raw.is_function {
                bail!(
                    "{}: 产品 recipe 的 TARGET 必须直接是 product(...)，不能是函数",
                    file.path.display()
                );
            }
            let value = raw
                .target_json
                .ok_or_else(|| anyhow!("{}: 缺少 TARGET", file.path.display()))?;
            let obj = value
                .as_object()
                .ok_or_else(|| anyhow!("{}: TARGET 必须是 dict", file.path.display()))?;
            if obj.get("kind").and_then(|v| v.as_str()) != Some("product") {
                bail!(
                    "{}: products/<产品>/<系列>.star 的 kind 必须是 product",
                    file.path.display()
                );
            }
            let series = obj
                .get("series")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("{}: product TARGET 缺少 series", file.path.display()))?;
            if Some(series) != file.series.as_deref() {
                bail!(
                    "{}: 文件名 {} 与 series={series} 不一致",
                    file.path.display(),
                    file.name
                );
            }
            if let Some(name) = obj.get("name").and_then(|v| v.as_str())
                && Some(name) != file.product.as_deref()
            {
                bail!(
                    "{}: TARGET.name={name} 与目录 {} 不一致",
                    file.path.display(),
                    file.product.clone().unwrap_or_default()
                );
            }
            bundled = obj
                .get("bundled")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .with_context(|| format!("{}: bundled 不合法", file.path.display()))?
                .unwrap_or_default();
        }

        metas.push(RecipeMeta {
            kind: file.kind,
            name: file.name,
            series: file.series,
            product: file.product,
            lines,
            product_lines,
            product_dep_lines,
            bundled,
        });
    }

    validate_meta_catalog(&metas, catalog)?;
    Ok(metas)
}

/// 第二阶段：按 ctx 构造具体 Target。
#[allow(clippy::too_many_arguments)]
pub fn load_targets(
    dir: &Path,
    catalog: &Catalog,
    product_name: &str,
    series: &str,
    product_version: &str,
    metas: &[RecipeMeta],
    lines: &BTreeMap<String, String>,
    base_ctx: &serde_json::Value,
) -> Result<Registry> {
    let defs = read_defs(dir)?;
    let globals = Globals::standard();
    let dialect = Dialect::Standard;
    let files = recipe_files(dir)?;
    let mut targets = Vec::new();
    let mut host_tools: BTreeMap<String, BTreeMap<String, FeatureCheck>> = BTreeMap::new();

    // 宿主事实注入 ctx：recipe 据此选择跟系统路径有关的构建参数（如 --openssldir）。
    let host = crate::host_commands::Env::load(dir)?;
    let mut base_ctx = base_ctx.clone();
    base_ctx["host"] = host.recipe_ctx();

    let product_meta = metas
        .iter()
        .find(|meta| {
            meta.kind == Kind::Product
                && meta.name == product_name
                && meta.series.as_deref() == Some(series)
        })
        .ok_or_else(|| anyhow!("没有找到产品 {product_name}@{series} 的 recipe 元数据"))?;
    let bundled = product_meta.bundled.clone();

    // 产品先加载：组件的默认挂载方式来自产品 profile。
    for file in files.iter().filter(|file| {
        file.kind == Kind::Product
            && file.name == product_name
            && file.series.as_deref() == Some(series)
    }) {
        let src = fs::read_to_string(&file.path)
            .with_context(|| format!("读不到 {}", file.path.display()))?;
        let prelude = read_prelude(&file.path)?;
        let mut ctx = base_ctx.clone();
        ctx["is_bundled"] = serde_json::Value::Bool(false);
        let json = eval_target(
            &file.path,
            &defs,
            prelude.as_deref(),
            &src,
            &globals,
            &dialect,
            &ctx,
        )?;
        let mut target: Target = serde_json::from_value(json)
            .with_context(|| format!("{}: TARGET 字段不合法", file.path.display()))?;
        if target.kind != Kind::Product {
            bail!("{}: TARGET.kind 必须是 product", file.path.display());
        }
        let artifact = catalog
            .artifact_for(product_name, series, product_version)
            .ok_or_else(|| anyhow!("catalog 里没有 {product_name} {product_version}"))?;
        target.name = product_name.to_owned();
        target.version = artifact.version.clone();
        target.url = artifact.url.clone();
        target.sha256 = artifact.sha256.clone();
        target.series = series.to_owned();
        target.line = series.to_owned();
        target.validate(&file.path)?;
        targets.push(target);
    }
    let product = targets
        .iter()
        .find(|t| t.kind == Kind::Product)
        .cloned()
        .ok_or_else(|| anyhow!("没有找到产品 {product_name}@{series} 的 recipe"))?;

    for file in files.iter().filter(|file| file.kind == Kind::Dep) {
        let src = fs::read_to_string(&file.path)
            .with_context(|| format!("读不到 {}", file.path.display()))?;
        let self_value = base_ctx
            .get("deps")
            .and_then(|deps| deps.get(&file.name))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        // 没绑定版本线的多线依赖库不加载；真实流程里消费它的组件会把线约束进来。
        if self_value.is_null() {
            continue;
        }
        let prelude = read_prelude(&file.path)?;
        let mut ctx = base_ctx.clone();
        ctx["is_bundled"] = serde_json::Value::Bool(false);
        ctx["self"] = self_value;
        let json = eval_target(
            &file.path,
            &defs,
            prelude.as_deref(),
            &src,
            &globals,
            &dialect,
            &ctx,
        )?;
        let mut target: Target = serde_json::from_value(json)
            .with_context(|| format!("{}: TARGET 字段不合法", file.path.display()))?;
        if target.kind != Kind::Dep {
            bail!("{}: TARGET.kind 与目录不一致", file.path.display());
        }
        if target.name != file.name {
            bail!(
                "{}: TARGET.name={} 与文件名 {} 不一致",
                file.path.display(),
                target.name,
                file.name
            );
        }
        target.line = lines.get(&file.name).cloned().unwrap_or_default();
        target.validate(&file.path)?;
        targets.push(target);
    }

    // 工具 recipe：catalog 有版本线就按版本构建；没有就是系统提供，只读 feature_checks。
    for file in files.iter().filter(|file| file.kind == Kind::Tool) {
        let src = fs::read_to_string(&file.path)
            .with_context(|| format!("读不到 {}", file.path.display()))?;
        let prelude = read_prelude(&file.path)?;
        let Some(lines) = catalog.lines(&file.name) else {
            let mut ctx = base_ctx.clone();
            ctx["is_bundled"] = serde_json::Value::Bool(false);
            ctx["self"] = serde_json::Value::Null;
            let json = eval_target(
                &file.path,
                &defs,
                prelude.as_deref(),
                &src,
                &globals,
                &dialect,
                &ctx,
            )?;
            let recipe: HostToolRecipe = serde_json::from_value(json)
                .with_context(|| format!("{}: TARGET 字段不合法", file.path.display()))?;
            if recipe.name != file.name {
                bail!(
                    "{}: TARGET.name={} 与文件名 {} 不一致",
                    file.path.display(),
                    recipe.name,
                    file.name
                );
            }
            validate_feature_checks(&recipe.feature_checks, &file.path)?;
            if !recipe.feature_checks.is_empty() {
                host_tools.insert(file.name.clone(), recipe.feature_checks);
            }
            continue;
        };
        // 一条线里的每个补丁都是可选版本；工具没有“产品系列”，
        // 每个版本各自成节点。
        let mut versions: Vec<(String, String, String, String)> = Vec::new();
        for (line, artifacts) in lines {
            for artifact in artifacts {
                versions.push((
                    line.clone(),
                    artifact.version.clone(),
                    artifact.url.clone(),
                    artifact.sha256.clone(),
                ));
            }
        }
        for (line, version, url, sha256) in versions {
            let mut ctx = base_ctx.clone();
            ctx["is_bundled"] = serde_json::Value::Bool(false);
            ctx["self"] = serde_json::json!({
                "version": version,
                "url": url,
                "sha256": sha256,
                "line": line,
            });
            let json = eval_target(
                &file.path,
                &defs,
                prelude.as_deref(),
                &src,
                &globals,
                &dialect,
                &ctx,
            )?;
            let mut target: Target = serde_json::from_value(json)
                .with_context(|| format!("{}: TARGET 字段不合法", file.path.display()))?;
            if target.kind != Kind::Tool {
                bail!("{}: TARGET.kind 与目录不一致", file.path.display());
            }
            if target.name != file.name {
                bail!(
                    "{}: TARGET.name={} 与文件名 {} 不一致",
                    file.path.display(),
                    target.name,
                    file.name
                );
            }
            target.version = version;
            target.url = url;
            target.sha256 = sha256;
            target.line = line;
            target.validate(&file.path)?;
            if !target.feature_checks.is_empty() {
                host_tools.insert(target.name.clone(), target.feature_checks.clone());
            }
            targets.push(target);
        }
    }

    for file in files.iter().filter(|file| {
        file.kind == Kind::Component && file.product.as_deref() == Some(product_name)
    }) {
        let is_bundled = bundled.iter().any(|item| item == &file.name);
        let src = fs::read_to_string(&file.path)
            .with_context(|| format!("读不到 {}", file.path.display()))?;
        let self_value = if is_bundled {
            serde_json::Value::Null
        } else {
            base_ctx
                .get("deps")
                .and_then(|deps| deps.get(&file.name))
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        };
        // 外部组件没有绑定版本线（多线且这次没选）时不加载。
        if !is_bundled && self_value.is_null() {
            continue;
        }
        let prelude = read_prelude(&file.path)?;
        let mut ctx = base_ctx.clone();
        ctx["is_bundled"] = serde_json::Value::Bool(is_bundled);
        ctx["self"] = self_value;
        let json = eval_target(
            &file.path,
            &defs,
            prelude.as_deref(),
            &src,
            &globals,
            &dialect,
            &ctx,
        )?;
        let mut target: Target = serde_json::from_value(json)
            .with_context(|| format!("{}: TARGET 字段不合法", file.path.display()))?;
        if target.kind != Kind::Component {
            bail!("{}: TARGET.kind 与目录不一致", file.path.display());
        }
        if target.name != file.name {
            bail!(
                "{}: TARGET.name={} 与文件名 {} 不一致",
                file.path.display(),
                target.name,
                file.name
            );
        }
        target.product = product_name.to_owned();
        target.source = if is_bundled {
            "bundled".to_owned()
        } else {
            "external".to_owned()
        };
        target.default_mode = default_mode_of(&product, &target, &file.path)?;
        target.line = lines.get(&file.name).cloned().unwrap_or_default();
        target.validate(&file.path)?;
        targets.push(target);
    }

    if !targets.iter().any(|t| t.kind == Kind::Product) {
        bail!("没有找到产品 {product_name}@{series} 的 recipe");
    }
    let mut reg = Registry::from_targets(targets)?;
    reg.host = host;
    reg.host_tools = host_tools;
    Ok(reg)
}

/// catalog 里没有版本线的工具 recipe：只能由系统提供，只声明具名检查。
#[derive(Debug, Deserialize)]
struct HostToolRecipe {
    name: String,
    #[serde(default)]
    feature_checks: BTreeMap<String, FeatureCheck>,
}

fn validate_feature_checks(checks: &BTreeMap<String, FeatureCheck>, path: &Path) -> Result<()> {
    let at = path.display();
    for (name, check) in checks {
        if name.is_empty() {
            bail!("{at}: feature_checks 里有空名字");
        }
        match &check.on_fail {
            CheckFail::NeedPackages { names, .. } => {
                if names.is_empty() {
                    bail!("{at}: feature_check {name} 的 on_fail.need_packages 不能为空");
                }
                if names.iter().any(|item| item.is_empty()) {
                    bail!("{at}: feature_check {name} 的 on_fail.need_packages 里有空名字");
                }
            }
            CheckFail::ErrorMsg { text } => {
                if text.trim().is_empty() {
                    bail!("{at}: feature_check {name} 的 on_fail.error_msg 不能为空");
                }
            }
        }
    }
    Ok(())
}

fn default_mode_of(product: &Target, component: &Target, path: &Path) -> Result<String> {
    let mode = product
        .profile
        .defaults
        .get(&component.source)
        .cloned()
        .or_else(|| component.modes.first().cloned())
        .ok_or_else(|| {
            anyhow!(
                "{}: component:{} 没有 modes，产品 {} 的 profile.defaults 也没有 {} 的默认方式",
                path.display(),
                component.name,
                product.name,
                component.source
            )
        })?;
    if !component.modes.iter().any(|m| m == &mode) {
        bail!(
            "{}: component:{} 的默认挂载方式 {mode} 不在 modes 里（{}）",
            path.display(),
            component.name,
            component.modes.join(", ")
        );
    }
    Ok(mode)
}

fn read_defs(dir: &Path) -> Result<String> {
    let path = dir.join("defs.star");
    fs::read_to_string(&path).with_context(|| format!("读不到 {}", path.display()))
}

struct RecipeFile {
    path: PathBuf,
    kind: Kind,
    name: String,
    /// 产品 / 组件所属的产品名。
    product: Option<String>,
    /// 产品系列。
    series: Option<String>,
}

fn recipe_files(dir: &Path) -> Result<Vec<RecipeFile>> {
    if dir.join("ext").is_dir() {
        bail!(
            "{} 还留着旧的 ext/ 目录；组件 recipe 已改到 components/<产品>/",
            dir.display()
        );
    }
    for path in star_files(dir)? {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if stem.starts_with("php") {
            bail!(
                "{} 是旧的产品 recipe 路径；已改到 products/php/<系列>.star",
                path.display()
            );
        }
    }

    let mut files = Vec::new();
    for file in star_files(&dir.join("dep"))? {
        files.push(classify(&file)?);
    }
    for file in star_files(&dir.join("tools"))? {
        files.push(classify(&file)?);
    }
    for (sub, kind) in [("components", Kind::Component), ("products", Kind::Product)] {
        let root = dir.join(sub);
        if !root.is_dir() {
            continue;
        }
        for entry in
            fs::read_dir(&root).with_context(|| format!("读不到目录 {}", root.display()))?
        {
            let product_dir = entry?.path();
            if !product_dir.is_dir() {
                continue;
            }
            for file in star_files(&product_dir)? {
                let stem = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                if stem.starts_with('_') {
                    continue;
                }
                let classified = classify(&file)?;
                if classified.kind != kind {
                    bail!("{}: 目录和 recipe 类型不一致", file.display());
                }
                files.push(classified);
            }
        }
    }
    if files.is_empty() {
        bail!("{} 下没有任何 recipe", dir.display());
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

fn star_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    if !dir.is_dir() {
        return Ok(v);
    }
    for entry in fs::read_dir(dir).with_context(|| format!("读不到目录 {}", dir.display()))? {
        let path = entry?.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("star") {
            v.push(path);
        }
    }
    v.sort();
    Ok(v)
}

fn classify(path: &Path) -> Result<RecipeFile> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("{}: 文件名不合法", path.display()))?;
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let grand = path
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if parent == "dep" {
        return Ok(RecipeFile {
            path: path.to_path_buf(),
            kind: Kind::Dep,
            name: stem.to_owned(),
            product: None,
            series: None,
        });
    }
    if parent == "tools" {
        return Ok(RecipeFile {
            path: path.to_path_buf(),
            kind: Kind::Tool,
            name: stem.to_owned(),
            product: None,
            series: None,
        });
    }
    match grand {
        "components" => Ok(RecipeFile {
            path: path.to_path_buf(),
            kind: Kind::Component,
            name: stem.to_owned(),
            product: Some(parent.to_owned()),
            series: None,
        }),
        "products" => Ok(RecipeFile {
            path: path.to_path_buf(),
            kind: Kind::Product,
            name: parent.to_owned(),
            product: Some(parent.to_owned()),
            series: Some(stem.to_owned()),
        }),
        _ => bail!(
            "{}: recipe 必须放在 dep/、tools/、components/<产品>/ 或 products/<产品>/ 下",
            path.display()
        ),
    }
}

fn prelude_name(path: &Path) -> String {
    path.parent()
        .map(|dir| dir.join("_profile.star").display().to_string())
        .unwrap_or_else(|| "_profile.star".to_owned())
}

fn read_prelude(path: &Path) -> Result<Option<String>> {
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let prelude = parent.join("_profile.star");
    if !prelude.is_file() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(&prelude).with_context(|| format!("读不到 {}", prelude.display()))?;
    Ok(Some(text))
}

fn parse_prelude(
    path: &Path,
    prelude: Option<&str>,
    dialect: &Dialect,
) -> Result<Option<AstModule>> {
    match prelude {
        Some(text) => Ok(Some(
            AstModule::parse(&prelude_name(path), text.to_owned(), dialect)
                .map_err(|e| anyhow!("{} 解析失败: {e}", prelude_name(path)))?,
        )),
        None => Ok(None),
    }
}

struct MetaRaw {
    target_json: Option<serde_json::Value>,
    lines_json: Option<serde_json::Value>,
    product_lines_json: Option<serde_json::Value>,
    product_dep_lines_json: Option<serde_json::Value>,
    is_function: bool,
}

fn eval_meta(
    path: &Path,
    defs: &str,
    prelude: Option<&str>,
    src: &str,
    globals: &Globals,
    dialect: &Dialect,
) -> Result<MetaRaw> {
    let name = path.display().to_string();
    let ast_defs = AstModule::parse("defs.star", defs.to_owned(), dialect)
        .map_err(|e| anyhow!("defs.star 解析失败: {e}"))?;
    let ast_prelude = parse_prelude(path, prelude, dialect)?;
    let ast = AstModule::parse(&name, src.to_owned(), dialect)
        .map_err(|e| anyhow!("{name} 解析失败: {e}"))?;

    Module::with_temp_heap(|module| {
        let mut eval = Evaluator::new(&module);
        eval.eval_module(ast_defs, globals)
            .map_err(|e| anyhow!("defs.star 求值失败: {e}"))?;
        if let Some(ast) = ast_prelude {
            eval.eval_module(ast, globals)
                .map_err(|e| anyhow!("{} 求值失败: {e}", prelude_name(path)))?;
        }
        eval.eval_module(ast, globals)
            .map_err(|e| anyhow!("{name} 求值失败: {e}"))?;
        let target = module
            .get("TARGET")
            .ok_or_else(|| anyhow!("{name} 没有定义顶层 TARGET"))?;
        let is_function = target.get_type() == "function";
        let target_json = if is_function {
            None
        } else {
            Some(
                target
                    .to_json_value()
                    .with_context(|| format!("{name}: TARGET 无法转成 JSON"))?,
            )
        };
        let lines_json = module
            .get("LINES")
            .map(|value| value.to_json_value())
            .transpose()
            .with_context(|| format!("{name}: LINES 无法转成 JSON"))?;
        let product_lines_json = module
            .get("PRODUCT_LINES")
            .map(|value| value.to_json_value())
            .transpose()
            .with_context(|| format!("{name}: PRODUCT_LINES 无法转成 JSON"))?;
        let product_dep_lines_json = module
            .get("PRODUCT_DEP_LINES")
            .map(|value| value.to_json_value())
            .transpose()
            .with_context(|| format!("{name}: PRODUCT_DEP_LINES 无法转成 JSON"))?;
        Ok(MetaRaw {
            target_json,
            lines_json,
            product_lines_json,
            product_dep_lines_json,
            is_function,
        })
    })
}

fn eval_target(
    path: &Path,
    defs: &str,
    prelude: Option<&str>,
    src: &str,
    globals: &Globals,
    dialect: &Dialect,
    ctx: &serde_json::Value,
) -> Result<serde_json::Value> {
    let name = path.display().to_string();
    let ast_defs = AstModule::parse("defs.star", defs.to_owned(), dialect)
        .map_err(|e| anyhow!("defs.star 解析失败: {e}"))?;
    let ast_prelude = parse_prelude(path, prelude, dialect)?;
    let ast = AstModule::parse(&name, src.to_owned(), dialect)
        .map_err(|e| anyhow!("{name} 解析失败: {e}"))?;
    let ctx_src = format!("ctx = {}", starlark_literal(ctx));
    let ast_ctx = AstModule::parse("<ctx>", ctx_src, dialect)
        .map_err(|e| anyhow!("{name}: 注入 ctx 失败: {e}"))?;

    Module::with_temp_heap(|module| {
        let mut eval = Evaluator::new(&module);
        eval.eval_module(ast_defs, globals)
            .map_err(|e| anyhow!("defs.star 求值失败: {e}"))?;
        if let Some(ast) = ast_prelude {
            eval.eval_module(ast, globals)
                .map_err(|e| anyhow!("{} 求值失败: {e}", prelude_name(path)))?;
        }
        eval.eval_module(ast_ctx, globals)
            .map_err(|e| anyhow!("{name}: 注入 ctx 失败: {e}"))?;
        eval.eval_module(ast, globals)
            .map_err(|e| anyhow!("{name} 求值失败: {e}"))?;

        let target = module
            .get("TARGET")
            .ok_or_else(|| anyhow!("{name} 没有定义顶层 TARGET"))?;
        let value = if target.get_type() == "function" {
            let ctx_value = module
                .get("ctx")
                .ok_or_else(|| anyhow!("{name}: TARGET 是函数，但 ctx 不存在"))?;
            eval.eval_function(target, &[ctx_value], &[])
                .map_err(|e| anyhow!("{name}: 调用 target(ctx) 失败: {e}"))?
        } else {
            target
        };
        value
            .to_json_value()
            .with_context(|| format!("{name}: TARGET 无法转成 JSON"))
    })
}

fn parse_product_lines(value: serde_json::Value) -> Result<BTreeMap<String, Vec<String>>> {
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("PRODUCT_LINES 必须是 dict"))?;
    let mut out = BTreeMap::new();
    for (series, value) in obj {
        let list = value
            .as_array()
            .ok_or_else(|| anyhow!("PRODUCT_LINES.{series} 必须是数组"))?;
        let lines: Vec<String> = list
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow!("PRODUCT_LINES.{series} 里必须是字符串"))
            })
            .collect::<Result<_>>()?;
        if lines.is_empty() {
            bail!("PRODUCT_LINES.{series} 不能为空");
        }
        out.insert(series.clone(), lines);
    }
    Ok(out)
}

fn parse_product_dep_lines(
    value: serde_json::Value,
) -> Result<BTreeMap<String, BTreeMap<String, Vec<String>>>> {
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("PRODUCT_DEP_LINES 必须是 dict"))?;
    let mut out = BTreeMap::new();
    for (series, value) in obj {
        let deps = value
            .as_object()
            .ok_or_else(|| anyhow!("PRODUCT_DEP_LINES.{series} 必须是 dict"))?;
        let mut parsed = BTreeMap::new();
        for (dep, value) in deps {
            let list = value
                .as_array()
                .ok_or_else(|| anyhow!("PRODUCT_DEP_LINES.{series}.{dep} 必须是数组"))?;
            let lines: Vec<String> = list
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| anyhow!("PRODUCT_DEP_LINES.{series}.{dep} 里必须是字符串"))
                })
                .collect::<Result<_>>()?;
            if lines.is_empty() {
                bail!("PRODUCT_DEP_LINES.{series}.{dep} 不能为空");
            }
            parsed.insert(dep.clone(), lines);
        }
        if parsed.is_empty() {
            bail!("PRODUCT_DEP_LINES.{series} 不能为空");
        }
        out.insert(series.clone(), parsed);
    }
    Ok(out)
}

fn parse_lines(value: serde_json::Value) -> Result<BTreeMap<String, LineMeta>> {
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("LINES 必须是 dict"))?;
    let mut out = BTreeMap::new();
    for (line, value) in obj {
        let line_obj = value
            .as_object()
            .ok_or_else(|| anyhow!("LINES.{line} 必须是 dict"))?;
        let product = line_obj
            .get("product")
            .map(|v| {
                v.as_array()
                    .ok_or_else(|| anyhow!("LINES.{line}.product 必须是数组"))?
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| anyhow!("LINES.{line}.product 必须是字符串"))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        let mut deps = BTreeMap::new();
        for (key, value) in line_obj {
            if key == "product" {
                continue;
            }
            let list = value
                .as_array()
                .ok_or_else(|| anyhow!("LINES.{line}.{key} 必须是数组"))?;
            let list: Vec<String> = list
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| anyhow!("LINES.{line}.{key} 里必须是字符串"))
                })
                .collect::<Result<_>>()?;
            deps.insert(key.clone(), list);
        }
        out.insert(line.clone(), LineMeta { product, deps });
    }
    Ok(out)
}

fn validate_meta_catalog(metas: &[RecipeMeta], catalog: &Catalog) -> Result<()> {
    let product_names: BTreeSet<String> = metas
        .iter()
        .filter(|m| m.kind == Kind::Product)
        .map(|m| m.name.clone())
        .collect();

    for name in &product_names {
        let recipe_series: BTreeSet<String> = metas
            .iter()
            .filter(|m| m.kind == Kind::Product && &m.name == name)
            .filter_map(|m| m.series.clone())
            .collect();
        let catalog_series: BTreeSet<String> = catalog
            .lines(name)
            .map(|l| l.keys().cloned().collect())
            .unwrap_or_default();
        if recipe_series != catalog_series {
            bail!(
                "catalog 的 {name} 系列与 recipe 不一致：recipe={recipe_series:?} catalog={catalog_series:?}"
            );
        }
    }

    for (name, lines) in &catalog.components {
        if product_names.contains(name) {
            continue;
        }
        let meta = metas
            .iter()
            .find(|m| m.kind == Kind::Dep && &m.name == name)
            .or_else(|| {
                metas
                    .iter()
                    .find(|m| m.kind == Kind::Component && &m.name == name)
            })
            .or_else(|| {
                metas
                    .iter()
                    .find(|m| m.kind == Kind::Tool && &m.name == name)
            })
            .ok_or_else(|| anyhow!("catalog 里的 {name} 没有 recipe"))?;
        if !meta.lines.is_empty() {
            let recipe_lines: BTreeSet<&String> = meta.lines.keys().collect();
            let catalog_lines: BTreeSet<&String> = lines.keys().collect();
            if recipe_lines != catalog_lines {
                bail!(
                    "{name} 的 LINES 与 catalog 不一致：recipe={recipe_lines:?} catalog={catalog_lines:?}"
                );
            }
        }
    }

    for meta in metas.iter().filter(|m| m.kind == Kind::Component) {
        let product = meta.product.clone().unwrap_or_default();
        let product_series: BTreeSet<String> = metas
            .iter()
            .filter(|m| m.kind == Kind::Product && m.name == product)
            .filter_map(|m| m.series.clone())
            .collect();
        if product_series.is_empty() {
            bail!(
                "组件 {} 的 product={product} 没有对应的产品 recipe",
                meta.name
            );
        }
        if !meta.lines.is_empty() && !catalog.components.contains_key(&meta.name) {
            bail!("{} 声明了 LINES，但 catalog 没有同名组件", meta.name);
        }
        for (line, lm) in &meta.lines {
            for series in &lm.product {
                if !product_series.contains(series) {
                    bail!(
                        "组件 {} 的 LINES.{line}.product 里有未知产品系列 {series}",
                        meta.name
                    );
                }
            }
        }
        if !meta.product_lines.is_empty() {
            let Some(component) = catalog.lines(&meta.name) else {
                bail!(
                    "{} 声明了 PRODUCT_LINES，但 catalog 没有同名组件",
                    meta.name
                );
            };
            for (series, allowed) in &meta.product_lines {
                if !product_series.contains(series) {
                    bail!("{} 的 PRODUCT_LINES 里有未知产品系列 {series}", meta.name);
                }
                if allowed.is_empty() {
                    bail!("{} 的 PRODUCT_LINES.{series} 不能为空", meta.name);
                }
                for line in allowed {
                    if !component.contains_key(line) {
                        bail!(
                            "{} 的 PRODUCT_LINES.{series} 引用了 catalog 里不存在的线 {line}",
                            meta.name
                        );
                    }
                }
            }
        }
        if meta.product_dep_lines.is_empty() {
            continue;
        }
        for (series, deps) in &meta.product_dep_lines {
            if !product_series.contains(series) {
                bail!(
                    "{} 的 PRODUCT_DEP_LINES 里有未知产品系列 {series}",
                    meta.name
                );
            }
            for (dep_name, allowed) in deps {
                if allowed.is_empty() {
                    bail!(
                        "{} 的 PRODUCT_DEP_LINES.{series}.{dep_name} 不能为空",
                        meta.name
                    );
                }
                let is_dep = metas
                    .iter()
                    .any(|m| m.kind == Kind::Dep && m.name == *dep_name);
                if !is_dep {
                    bail!(
                        "{} 的 PRODUCT_DEP_LINES.{series}.{dep_name} 不是 dep recipe",
                        meta.name
                    );
                }
                let Some(component) = catalog.lines(dep_name) else {
                    bail!(
                        "{} 的 PRODUCT_DEP_LINES 引用了 catalog 里不存在的 dep {dep_name}",
                        meta.name
                    );
                };
                for line in allowed {
                    if !component.contains_key(line) {
                        bail!(
                            "{} 的 PRODUCT_DEP_LINES.{series}.{dep_name} 引用了 catalog 里不存在的线 {line}",
                            meta.name
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

fn starlark_literal(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "None".to_owned(),
        serde_json::Value::Bool(true) => "True".to_owned(),
        serde_json::Value::Bool(false) => "False".to_owned(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => starlark_string(s),
        serde_json::Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(starlark_literal)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        serde_json::Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}: {}", starlark_string(k), starlark_literal(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn starlark_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bind;
    use crate::bundle::Bundle;

    fn dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes")
    }

    fn load(
        series: &str,
        selected: &[&str],
        pins: &[(&str, &str)],
    ) -> Result<(Registry, bind::Bindings)> {
        load_product("php", series, selected, pins)
    }

    fn load_product(
        product: &str,
        series: &str,
        selected: &[&str],
        pins: &[(&str, &str)],
    ) -> Result<(Registry, bind::Bindings)> {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml"))?;
        let metas = load_meta(&dir, &catalog)?;
        let bundle = Bundle::load(
            &dir.join("bundles")
                .join(product)
                .join(format!("{series}.toml")),
        )?;
        let mut pin_map = BTreeMap::new();
        for (name, line) in pins {
            pin_map.insert((*name).to_owned(), (*line).to_owned());
        }
        let selected: Vec<String> = selected.iter().map(|s| (*s).to_owned()).collect();
        let mut selected = selected;
        for name in bundle.default_components.keys() {
            if !selected.contains(name) {
                selected.push(name.clone());
            }
        }
        let bindings = bind::resolve(
            &catalog,
            &bundle,
            &metas,
            product,
            series,
            &selected,
            &crate::lock::EffectivePins {
                lines: pin_map,
                versions: BTreeMap::new(),
            },
        )?;
        let version = catalog.product_version_for(product, series, series)?;
        let ctx = bind::context(product, series, &version, &bindings);
        let reg = load_targets(
            &dir,
            &catalog,
            product,
            series,
            &version,
            &metas,
            &bindings.lines,
            &ctx,
        )?;
        Ok((reg, bindings))
    }

    #[test]
    fn openssl_ssl_dir_follows_the_host_family() {
        let family = crate::host_commands::Env::load(&dir()).unwrap().family();
        let expected = match family {
            "rpm" => "--openssldir=/etc/pki/tls",
            "deb" => "--openssldir=/etc/ssl",
            _ => "--openssldir={prefix}/ssl",
        };
        for series in ["7.4", "8.5"] {
            let (reg, _) = load(series, &[], &[]).unwrap();
            let openssl = reg.get("dep:openssl").unwrap();
            assert!(
                openssl.configure.iter().any(|arg| arg == expected),
                "{series}: {:?}",
                openssl.configure
            );
        }
    }

    #[test]
    fn bundled_facts_come_from_the_product_recipe() {
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let product = reg.product_any().unwrap();
        assert!(product.bundled.contains(&"openssl".to_owned()));
        assert!(product.bundled.contains(&"libxml".to_owned()));
        assert_eq!(
            product.bundled_disable.get("libxml").map(String::as_str),
            Some("--without-libxml")
        );
        assert_eq!(reg.component("openssl").unwrap().source, "bundled");
        assert!(reg.component("openssl").unwrap().url.is_empty());
        assert_eq!(reg.component("swoole").unwrap().source, "external");
        assert!(!reg.component("swoole").unwrap().url.is_empty());
    }

    #[test]
    fn php_profile_is_shared_and_series_can_extend_it() {
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let product = reg.product_any().unwrap();
        assert!(!product.profile.tools.iter().any(|tool| tool.name == "gcc"));
        let toolchain = product.toolchain.as_ref().expect("8.5 要声明 toolchain");
        assert_eq!(toolchain.gcc.len(), 1);
        assert_eq!(toolchain.gcc[0].min, "9");
        assert!(!product.profile.post_install.is_empty());
        assert_eq!(product.profile.entry_template, "entry.sh");
        assert_eq!(product.profile.env["CC"], "gcc");
        assert_eq!(product.profile.register_dir("php-ini"), Some("etc/conf.d"));
        assert_eq!(product.profile.register_dir("site-packages"), None);

        let (reg, _) = load("8.0", &[], &[]).unwrap();
        let product = reg.product_any().unwrap();
        assert_eq!(product.profile.env["CC"], "gcc");
        let toolchain = product.toolchain.as_ref().expect("8.0 要声明 toolchain");
        assert_eq!(
            toolchain.gcc.last().unwrap().cflags,
            "-std=gnu17 -Wno-error=incompatible-pointer-types"
        );
    }

    #[test]
    fn php_targets_are_declared_per_series() {
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let product = reg.product_any().unwrap();
        for name in ["cli", "fpm", "cgi"] {
            assert!(product.targets.contains_key(name), "缺少 target {name}");
        }
        assert_eq!(product.targets["cli"].artifacts, vec!["bin/php".to_owned()]);
        assert_eq!(
            product.targets["fpm"].artifacts,
            vec!["sbin/php-fpm".to_owned()]
        );
        assert!(
            product.targets["fpm"].disable_flags.is_empty(),
            "PHP 没有 --disable-fpm，不能生成禁用参数"
        );
        assert_eq!(
            product.targets["fpm"].config["user"].flags,
            "--with-fpm-user={value}"
        );
    }

    #[test]
    fn php_bundle_default_targets_are_declared() {
        for series in ["7.4", "8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"] {
            let (reg, _) = load(series, &[], &[]).unwrap();
            let product = reg.product_any().unwrap();
            let bundle =
                crate::bundle::Bundle::load(&dir().join(format!("bundles/php/{series}.toml")))
                    .unwrap();
            assert!(
                !bundle.targets_default.is_empty(),
                "{series} 没有默认 targets"
            );
            for name in &bundle.targets_default {
                assert!(
                    product.targets.contains_key(name),
                    "{series} 的默认 target {name} 没有声明"
                );
            }
        }
    }

    #[test]
    fn nginx_profile_is_shared() {
        let (reg, _) = load_product("nginx", "1.29", &[], &[]).unwrap();
        let product = reg.product_any().unwrap();
        assert!(product.profile.tools.iter().any(|tool| tool.name == "perl"));
        assert_eq!(product.profile.entry, "");
        assert!(product.profile.post_install.is_empty());
    }

    #[test]
    fn php_profile_declares_ini_backup() {
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let backup = reg
            .product_any()
            .unwrap()
            .profile
            .backup
            .as_ref()
            .expect("PHP 应该声明 backup");
        assert_eq!(backup.relative_dir, "backup");
        assert_eq!(backup.name, "{timestamp}_{component}.ini");
    }

    #[test]
    fn nginx_profile_has_no_backup() {
        let (reg, _) = load_product("nginx", "1.29", &[], &[]).unwrap();
        assert!(reg.product_any().unwrap().profile.backup.is_none());
    }

    #[test]
    fn backup_policy_rejects_bad_templates() {
        let policy = |relative_dir: &str, name: &str| BackupPolicy {
            relative_dir: relative_dir.into(),
            name: name.into(),
        };
        policy("backup", "{timestamp}_{component}.ini")
            .validate("test")
            .unwrap();
        policy("backup/php", "{component}-{mode}.ini")
            .validate("test")
            .unwrap();
        for bad in [
            policy("", "{component}.ini"),
            policy("../x", "{component}.ini"),
            policy("backup", "fixed.ini"),
            policy("backup", "{component}/x.ini"),
            policy("backup", "{unknown}.ini"),
        ] {
            assert!(bad.validate("test").is_err(), "{bad:?}");
        }
    }

    #[test]
    fn nginx_profile_has_no_php_env_or_prepare() {
        let (reg, _) = load_product("nginx", "1.29", &["lua-nginx-module"], &[]).unwrap();
        let product = reg.product_any().unwrap();
        let nginx_component = reg.component("lua-nginx-module").unwrap();
        for key in product.profile.env.keys() {
            assert!(!key.starts_with("PHP_"), "nginx profile 不应有 {key}");
        }
        for mode in product.profile.modes.values() {
            for step in &mode.steps {
                assert!(!step.contains("phpize"), "nginx 步骤不应含 phpize: {step}");
            }
        }

        // PHP profile 反过来要有 phpize 步骤；autoconf 走 PATH，不用环境变量。
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let product = reg.product_any().unwrap();
        assert!(!product.profile.env.contains_key("PHP_AUTOCONF"));
        assert!(product.profile.tools.iter().any(|tool| tool.name == "m4"));
        let shared = product.profile.modes.get("shared").unwrap();
        assert!(shared.steps.iter().any(|step| step.contains("phpize")));

        assert!(nginx_component.prepare.is_empty());
    }

    #[test]
    fn php_bundled_extensions_declare_config_m4_prepare() {
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        for name in ["openssl", "zlib"] {
            let component = reg.component(name).unwrap();
            assert!(
                component
                    .prepare
                    .iter()
                    .any(|line| line.contains("config0.m4") && line.contains("config.m4")),
                "{name}: {:?}",
                component.prepare
            );
        }

        let (reg, _) = load_product("nginx", "1.29", &[], &[]).unwrap();
        let component = reg.component("lua-nginx-module").unwrap();
        assert!(
            !component
                .prepare
                .iter()
                .any(|line| line.contains("config0.m4"))
        );
    }

    #[test]
    fn default_selection_comes_from_the_bundle_table() {
        let dir = dir();
        let bundle = Bundle::load(&dir.join("bundles/php/8.5.toml")).unwrap();
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let sel = crate::bundle::selection(&reg, &bundle, "8.5").unwrap();
        assert_eq!(
            sel.mode_of(reg.component("curl").unwrap()),
            "static".to_owned()
        );
        assert_eq!(
            sel.per_component["curl"].deps["libcurl"],
            vec!["ssl".to_owned()]
        );
        assert_eq!(
            sel.features_of(reg.component("openssl").unwrap()),
            Some(&BTreeSet::from(["argon2".to_owned()]))
        );
    }

    #[test]
    fn argon2_requires_the_right_series_and_openssl_line() {
        let (reg, _) = load("8.5", &["openssl"], &[]).unwrap();
        assert!(
            reg.component("openssl")
                .unwrap()
                .features
                .contains_key("argon2")
        );
        assert!(Selection::parse(&reg, &["openssl=feature:[argon2]".into()]).is_ok());

        let (reg, _) = load("8.5", &["openssl"], &[("openssl", "3.0")]).unwrap();
        let err = Selection::parse(&reg, &["openssl=feature:[argon2]".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("需要 dep:openssl 3.5, 4.0，当前 3.0"), "{err}");

        let (reg, _) = load("8.2", &["openssl"], &[]).unwrap();
        let err = Selection::parse(&reg, &["openssl=feature:[argon2]".into()])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("需要产品系列 8.4, 8.5, 8.6，当前 8.2"),
            "{err}"
        );
    }

    #[test]
    fn feature_selection_is_validated_and_can_be_cleared() {
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let sel = Selection::parse(&reg, &["openssl=feature:[argon2]".into()]).unwrap();
        assert_eq!(
            sel.features_of(reg.component("openssl").unwrap()),
            Some(&BTreeSet::from(["argon2".to_owned()]))
        );

        let sel = Selection::parse(&reg, &["openssl=feature:[]".into()]).unwrap();
        assert_eq!(
            sel.features_of(reg.component("openssl").unwrap()),
            Some(&BTreeSet::from([]))
        );

        let err = Selection::parse(&reg, &["openssl=feature:argon2".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("必须写方括号"), "{err}");
    }

    #[test]
    fn every_series_builds_with_bundle_defaults() {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        for series in catalog.lines("php").unwrap().keys() {
            let series = series.clone();
            // 8.6 还没适配；swoole / xdebug 也没有支持 8.6 的版本线，不在这里验收。
            if series == "8.6" {
                continue;
            }
            let (reg, bindings) = load(&series, &["swoole", "openssl", "xdebug"], &[]).unwrap();
            let product = reg.product_any().unwrap();
            assert_eq!(
                product.version,
                catalog
                    .product_version_for("php", &series, &series)
                    .unwrap()
            );
            assert_eq!(product.line, series);
            assert!(reg.get("dep:swoole").is_none());
            let swoole = reg.component("swoole").unwrap();
            assert_eq!(swoole.line, bindings.lines["swoole"]);
            assert_eq!(swoole.version, bindings.artifacts["swoole"].version);
            assert!(
                swoole.needs.contains(&"openssl".to_owned()),
                "{series} swoole 应该依赖 openssl"
            );
            assert_eq!(
                reg.get("dep:openssl").unwrap().line,
                bindings.lines["openssl"]
            );
            assert_eq!(
                reg.get("dep:libcurl").unwrap().version,
                catalog
                    .get("libcurl", bindings.lines["libcurl"].as_str())
                    .unwrap()
                    .version
            );
        }
    }

    #[test]
    fn bundle_dep_line_conflict_with_a_component_is_an_error() {
        let err = load("8.2", &["swoole"], &[("swoole", "4.8")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("bundle 里 openssl = 3.5"), "{err}");
        assert!(err.contains("1.1.1"), "{err}");
    }

    #[test]
    fn hard_pin_conflict_is_an_error() {
        let err = load("8.2", &["swoole"], &[("swoole", "4.8"), ("openssl", "3.5")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("冲突"), "{err}");
    }

    #[test]
    fn unsupported_product_line_is_an_error() {
        let err = load("8.5", &["swoole"], &[("swoole", "4.8")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("不支持产品 8.5"), "{err}");
    }

    #[test]
    fn preparse_reads_version_option() {
        let raw =
            preparse(&["swoole=mode:shared,dep:libcurl[ssl],version:6.2".to_owned()]).unwrap();
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].name, "swoole");
        assert_eq!(raw[0].version.as_deref(), Some("6.2"));
    }

    #[test]
    fn preparse_rejects_two_components_in_one_spec() {
        let err = preparse(&["curl,openssl=version:4.0".to_owned()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("一次只能指定一个组件"), "{err}");
    }

    #[test]
    fn gd_preset_is_used_for_explicit_selection() {
        let dir = dir();
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let bundle = Bundle::load(&dir.join("bundles/php/8.5.toml")).unwrap();
        let presets = crate::bundle::presets(&reg, &bundle, "8.5").unwrap();
        assert_eq!(
            presets.per_component["gd"].features,
            Some(BTreeSet::from([
                "freetype".to_owned(),
                "jpeg".to_owned(),
                "webp".to_owned(),
            ]))
        );
        assert_eq!(
            presets.per_component["openssl"].features,
            Some(BTreeSet::from(["argon2".to_owned()]))
        );
        assert_eq!(
            presets.per_component["curl"].deps["libcurl"],
            vec!["ssl".to_owned()]
        );
    }

    #[test]
    fn default_component_ref_uses_preset_and_local_mode() {
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let path =
            std::env::temp_dir().join(format!("phprt-bundle-gd-ref-{}.toml", std::process::id()));
        std::fs::write(
            &path,
            r#"
product = "php"
[component_presets]
gd = { features = ["jpeg", "webp", "freetype"] }
[default_components]
gd = { ref = "component_presets.gd", mode = "shared" }
"#,
        )
        .unwrap();
        let bundle = Bundle::load(&path).unwrap();
        let sel = crate::bundle::selection(&reg, &bundle, "8.5").unwrap();
        assert_eq!(
            sel.per_component["gd"].features,
            Some(BTreeSet::from([
                "freetype".to_owned(),
                "jpeg".to_owned(),
                "webp".to_owned(),
            ]))
        );
        assert_eq!(sel.per_component["gd"].mode.as_deref(), Some("shared"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn gd_loads_for_series_with_bundled_facts() {
        for series in ["8.0", "8.1", "8.2", "8.3", "8.4", "8.5", "8.6"] {
            let (reg, _) = load(series, &[], &[]).unwrap();
            assert!(reg.get("component:gd").is_some(), "{series} 应该加载 gd");
        }

        let (reg, _) = load("7.4", &[], &[]).unwrap();
        assert!(reg.get("component:gd").is_none(), "7.4 不该加载 gd");
    }

    #[test]
    fn zlib_is_bundled_for_minimal_series() {
        // 7.4 的 bundled 只列了部分内置扩展；zlib 必须在里面，
        // 否则会被 catalog 的同名库记录当成外部 PHP 扩展。
        for series in ["7.4", "8.6"] {
            let (reg, _) = load(series, &[], &[]).unwrap();
            let zlib = reg.component("zlib").unwrap();
            assert_eq!(zlib.source, "bundled", "{series}");
        }
    }

    #[test]
    fn default_components_are_the_same_for_adapted_series() {
        let dir = dir();
        let mut keys: Option<BTreeSet<String>> = None;
        for series in ["8.3", "8.4", "8.5"] {
            let bundle = Bundle::load(
                &dir.join("bundles")
                    .join("php")
                    .join(format!("{series}.toml")),
            )
            .unwrap_or_else(|e| panic!("{series} bundle: {e}"));
            let this: BTreeSet<String> = bundle.default_components.keys().cloned().collect();
            match &keys {
                None => keys = Some(this),
                Some(first) => assert_eq!(
                    first,
                    &this,
                    "{series} 的默认集和已验收系列不一致：{:?}",
                    this.symmetric_difference(first).collect::<Vec<_>>()
                ),
            }
        }
    }

    #[test]
    fn gd_product_dep_lines_cover_every_series() {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        let mut metas = load_meta(&dir, &catalog).unwrap();
        let gd = metas
            .iter()
            .find(|m| m.kind == Kind::Component && m.name == "gd")
            .unwrap();
        for series in catalog.lines("php").unwrap().keys() {
            assert!(
                gd.product_dep_lines.contains_key(series),
                "gd 的 PRODUCT_DEP_LINES 缺少 {series}"
            );
        }
        assert_eq!(gd.product_dep_lines["8.5"]["jpeg"], vec!["3.1".to_owned()]);

        let gd = metas
            .iter_mut()
            .find(|m| m.kind == Kind::Component && m.name == "gd")
            .unwrap();
        gd.product_dep_lines
            .insert("9.9".to_owned(), BTreeMap::new());
        let err = validate_meta_catalog(&metas, &catalog)
            .unwrap_err()
            .to_string();
        assert!(err.contains("PRODUCT_DEP_LINES"), "{err}");
    }

    #[test]
    fn gd_product_dep_lines_reject_other_lines() {
        let dir = dir();
        let mut catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        catalog.components.get_mut("jpeg").unwrap().insert(
            "4.0".into(),
            vec![crate::catalog::Artifact {
                version: "4.0.0".into(),
                url: "https://example.com/jpeg-4.0.0.tar.gz".into(),
                sha256: String::new(),
            }],
        );
        let metas = load_meta(&dir, &catalog).unwrap();
        let bundle = Bundle::load(&dir.join("bundles/php/8.5.toml")).unwrap();
        let mut pins = BTreeMap::new();
        pins.insert("jpeg".to_owned(), "4.0".to_owned());
        let err = crate::bind::resolve(
            &catalog,
            &bundle,
            &metas,
            "php",
            "8.5",
            &["gd".into()],
            &crate::lock::EffectivePins {
                lines: pins,
                versions: BTreeMap::new(),
            },
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("jpeg 4.0 与当前产品/组件选择冲突"), "{err}");
    }

    #[test]
    fn gd_dependencies_are_bound_and_ordered() {
        let (mut reg, bindings) = load("8.5", &["gd"], &[]).unwrap();
        assert_eq!(bindings.lines["jpeg"], "3.1");
        assert_eq!(bindings.lines["libpng"], "1.6");
        assert_eq!(bindings.lines["freetype"], "2.13");

        let sel = Selection::parse(&reg, &["gd=feature:[jpeg,webp,freetype]".into()]).unwrap();
        let features = sel.features(&reg);
        let product = reg.product_any().unwrap();
        let product_id = product.id();
        reg.targets.get_mut(&product_id).unwrap().components = vec!["gd".into()];
        let order = crate::plan::resolve(&reg, &product_id, &sel, &features).unwrap();
        let ids: Vec<String> = order.iter().map(|t| t.id()).collect();
        let pos = |id: &str| ids.iter().position(|x| x == id).unwrap();
        assert!(pos("dep:zlib") < pos("dep:libpng"));
        assert!(pos("dep:libpng") < pos("dep:freetype"));
        assert!(pos("dep:libpng") < pos("dep:jpeg"));
        assert!(pos("component:gd") < pos(&product_id));
        assert!(pos("dep:jpeg") < pos("component:gd"));
    }

    #[test]
    fn feature_commands_are_checked() {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        let metas = load_meta(&dir, &catalog).unwrap();
        let bindings = crate::bind::resolve(
            &catalog,
            &Bundle::load(&dir.join("bundles/php/8.5.toml")).unwrap(),
            &metas,
            "php",
            "8.5",
            &[],
            &crate::lock::EffectivePins::default(),
        )
        .unwrap();
        let version = catalog.product_version_for("php", "8.5", "8.5").unwrap();
        let ctx = crate::bind::context("php", "8.5", &version, &bindings);
        let mut reg = load_targets(
            &dir,
            &catalog,
            "php",
            "8.5",
            &version,
            &metas,
            &bindings.lines,
            &ctx,
        )
        .unwrap();
        let id = "dep:jpeg".to_owned();
        let dep = reg.targets.get_mut(&id).unwrap();
        dep.features.insert(
            "missing".into(),
            Feature {
                requires: Requires {
                    commands: vec!["definitely-not-a-phprt-command".into()],
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let err = feature_available(&reg, reg.get(&id).unwrap(), "missing")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("需要命令 definitely-not-a-phprt-command"),
            "{err}"
        );
    }

    #[test]
    fn component_openssl_product_lines_are_enforced() {
        let (_, bindings) = load("7.4", &["openssl"], &[]).unwrap();
        assert_eq!(bindings.lines["openssl"], "1.1.1");

        let err = load("7.4", &["openssl"], &[("openssl", "3.5")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("冲突"), "{err}");

        let (_, bindings) = load("8.5", &["openssl"], &[("openssl", "4.0")]).unwrap();
        assert_eq!(bindings.lines["openssl"], "4.0");
    }

    #[test]
    fn dep_recipes_have_no_product_relationship() {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        let metas = load_meta(&dir, &catalog).unwrap();
        let dep = metas
            .iter()
            .find(|m| m.kind == Kind::Dep && m.name == "openssl")
            .unwrap();
        assert!(dep.lines.is_empty());
        assert!(dep.product_lines.is_empty());

        let component = metas
            .iter()
            .find(|m| m.kind == Kind::Component && m.name == "openssl")
            .unwrap();
        assert_eq!(
            component.product_lines["8.5"],
            vec!["1.1.1", "3.0", "3.5", "4.0"]
        );
    }

    #[test]
    fn dep_recipe_cannot_declare_product_relations() {
        let dir = std::env::temp_dir().join(format!("phprt-dep-lines-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dep")).unwrap();
        std::fs::write(dir.join("defs.star"), "").unwrap();
        std::fs::write(
            dir.join("dep/foo.star"),
            r#"
LINES = {"1": {"product": ["8.5"]}}

def target(ctx):
    return {"kind": "dep", "name": "foo", "version": "1", "url": "https://example.com/foo.tar.gz"}

TARGET = target
"#,
        )
        .unwrap();
        let err = load_meta(&dir, &Catalog::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("dep recipe 不能声明"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_recipe_layout_is_rejected() {
        let dir = std::env::temp_dir().join(format!("phprt-old-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ext")).unwrap();
        std::fs::write(dir.join("defs.star"), "").unwrap();
        std::fs::write(dir.join("php8.5.star"), "TARGET = {}").unwrap();
        let err = load_meta(&dir, &Catalog::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("旧的 ext/ 目录"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn catalog_rejects_unknown_versions() {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        assert_eq!(catalog.find_product_series("php", "8.5").unwrap(), "8.5");
        assert_eq!(
            catalog.find_product_series("php", "php@8.5.12").unwrap(),
            "8.5"
        );
        assert!(catalog.find_product_series("php", "8.9").is_err());
    }
    #[test]
    fn target_rejects_unknown_fields() {
        let value = serde_json::json!({
            "kind": "dep",
            "name": "demo",
            "version": "1",
            "url": "https://example.com/demo.tar.gz",
            "host_tools": ["cmake"],
        });
        assert!(serde_json::from_value::<Target>(value).is_err());
    }

    #[test]
    fn tool_feature_checks_are_parsed() {
        let spec: ToolSpec = serde_json::from_value(serde_json::json!({
            "name": "perl",
            "min": "5.20",
            "feature_checks": ["findbin"],
        }))
        .unwrap();
        assert_eq!(spec.feature_checks, vec!["findbin".to_owned()]);
    }

    #[test]
    fn compilers_are_toolchain_not_tools() {
        for name in ["gcc", "g++", "gxx", "cc", "c++"] {
            let tools = vec![ToolSpec {
                name: name.to_owned(),
                min: "9".to_owned(),
                max: String::new(),
                probe: vec!["--version".to_owned()],
                feature_checks: Vec::new(),
            }];
            let err = validate_tools(&tools, "fixture").unwrap_err().to_string();
            assert!(err.contains("toolchain"), "{err}");
        }
    }

    #[test]
    fn preparse_scheme_reads_many_components_and_version_lines() {
        let raw = preparse_scheme(
            "curl=mode:shared,dep:libcurl[ssl,nghttp2],zip=dep:libzip[encryption],swoole=version:6.2",
        )
        .unwrap();
        let names: Vec<&str> = raw.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, vec!["curl", "zip", "swoole"]);
        assert_eq!(raw[2].version.as_deref(), Some("6.2"));

        // 单组件 spec 仍然只接受一个组件。
        let err = preparse(&["a,b".to_owned()]).unwrap_err().to_string();
        assert!(err.contains("一次只能指定一个组件"), "{err}");
    }
}
