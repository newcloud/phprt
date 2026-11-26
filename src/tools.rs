//! 构建期工具：解析（宿主 / build/tools / catalog）和版本探测。
//!
//! 解析按节点进行，同名工具允许在同一次构建里出现多个版本。宿主优先；
//! `--lock` 时 state 里记录的 built 版本优先，保证结果可复现。

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::layout;
use crate::recipe::{CheckFail, Kind, Registry, Selection, Strategy, Target, ToolSpec};

/// 工具从哪里来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Host,
    Store,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Host => "host",
            Provider::Store => "store",
        }
    }
}

/// 一个节点实际使用的工具。
#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub name: String,
    pub version: String,
    pub provider: Provider,
    /// built 工具的安装前缀；host 为空。
    pub prefix: Option<PathBuf>,
}

/// 解析不出来的原因；只有“缺命令”会进安装提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedKind {
    /// 命令不在 PATH，catalog 里也没有这个工具的 recipe。
    MissingCommand,
    /// 命令在，但版本不在要求区间，catalog 也没有可用版本。
    OutOfRange,
    /// catalog 有 recipe 但版本不满足，或探测失败等数据问题。
    Data,
}

impl UnresolvedKind {
    pub fn is_missing_command(self) -> bool {
        matches!(self, Self::MissingCommand)
    }
}

/// 带分类的解析失败；`lenient` 模式据此决定是提示安装还是保持原错误。
#[derive(Debug)]
struct ToolError {
    kind: UnresolvedKind,
    packages: Vec<String>,
    /// feature_check 里写的说明（`need_packages(..., reason = ...)`）；空表示没写。
    note: String,
    message: String,
}

impl ToolError {
    fn boxed(
        kind: UnresolvedKind,
        packages: Vec<String>,
        note: String,
        message: String,
    ) -> anyhow::Error {
        anyhow::Error::new(Self {
            kind,
            packages,
            note,
            message,
        })
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolError {}

/// 解析不出来的工具；plan 只展示，不因此失败。
#[derive(Debug, Clone)]
pub struct Unresolved {
    pub name: String,
    pub reason: String,
    pub kind: UnresolvedKind,
    /// 需要装包的键（feature_check 的 need_packages）；空表示用 `name`。
    pub packages: Vec<String>,
    /// 工具 recipe 里写的说明；空表示没写。
    pub note: String,
}

/// 一次构建的工具解析结果。
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    /// 节点 id -> 实际使用的工具（同名只留一个版本）。
    pub per_node: BTreeMap<String, Vec<ResolvedTool>>,
    /// 需要构建的工具节点 id，已按依赖顺序排好。
    pub tool_ids: Vec<String>,
    pub unresolved: Vec<Unresolved>,
}

impl Resolution {
    /// 本节点解析到的 built 工具安装前缀。
    pub fn prefixes(&self, node: &str) -> Vec<PathBuf> {
        self.per_node
            .get(node)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| tool.prefix.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn tools_of(&self, node: &str) -> &[ResolvedTool] {
        self.per_node.get(node).map(Vec::as_slice).unwrap_or(&[])
    }

    /// 所有工具的展示项，按名字 / 版本去重。
    pub fn report(&self) -> Vec<&ResolvedTool> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for tools in self.per_node.values() {
            for tool in tools {
                if seen.insert((tool.name.clone(), tool.version.clone(), tool.provider)) {
                    out.push(tool);
                }
            }
        }
        out.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        out
    }
}

/// 解析时用到的上下文。
pub struct Options<'a> {
    pub reg: &'a Registry,
    pub product: &'a Target,
    pub sel: &'a Selection,
    pub build_root: &'a Path,
    /// `--lock`：优先复用 state 里记录过的 built 工具版本。
    pub locked: BTreeMap<String, Vec<String>>,
    pub lock: bool,
    /// plan：解析失败只记录，不中断。
    pub lenient: bool,
}

/// 解析整条构建顺序里的工具。
pub fn resolve(order: &[&Target], opts: &Options<'_>) -> Result<Resolution> {
    let mut resolver = Resolver {
        opts,
        tools_dir: layout::tools_dir(opts.build_root),
        host: BTreeMap::new(),
        store: BTreeMap::new(),
        out: Resolution::default(),
        seen: BTreeSet::new(),
    };
    for node in order {
        let specs = requirements(opts.reg, opts.sel, opts.product, node)?;
        if specs.is_empty() {
            continue;
        }
        let specs = merge(specs, &node.id())?;
        let mut resolved = Vec::new();
        for spec in &specs {
            match resolver.one(spec, 0) {
                Ok(tool) => resolved.push(tool),
                Err(reason) => {
                    if !opts.lenient {
                        return Err(reason);
                    }
                    let message = format!("{reason:#}");
                    resolver.out.unresolved.push(Unresolved {
                        name: spec.name.clone(),
                        reason: message,
                        kind: classify(&reason),
                        packages: packages_of(&reason),
                        note: note_of(&reason),
                    });
                }
            }
        }
        if !resolved.is_empty() {
            resolver.out.per_node.insert(node.id(), resolved);
        }
    }
    Ok(resolver.out)
}

/// 一个节点需要解析的工具要求。
///
/// 产品 profile 的 tools 是产品 / 依赖 / 组件的基线；工具节点只用自己的
/// tools，否则工具无法跨产品共享（不同产品会把它绑回各自的依赖树）。
fn requirements(
    reg: &Registry,
    sel: &Selection,
    product: &Target,
    node: &Target,
) -> Result<Vec<ToolSpec>> {
    let mut out = Vec::new();
    if node.kind != Kind::Tool {
        out.extend(product.profile.tools.iter().cloned());
    }
    match node.kind {
        Kind::Product => {}
        Kind::Dep | Kind::Tool => out.extend(node.tools.iter().cloned()),
        Kind::Component => {
            // 只有 standalone 组件需要 autoconf / phpize 这类构建工具。
            let mode = sel.mode_of(node);
            if reg.product_mode(node, &mode)?.strategy_kind()? == Strategy::Standalone {
                out.extend(node.tools.iter().cloned());
            }
        }
    }
    Ok(out)
}

/// 同名声明的合并：min 取大、max 取小；空表示不限制。
fn merge(specs: Vec<ToolSpec>, node: &str) -> Result<Vec<ToolSpec>> {
    let mut out: Vec<ToolSpec> = Vec::new();
    for spec in specs {
        let Some(existing) = out.iter_mut().find(|item| item.name == spec.name) else {
            out.push(spec);
            continue;
        };
        if !spec.min.is_empty()
            && (existing.min.is_empty()
                || crate::version::compare_to_bound(&spec.min, &existing.min)
                    == std::cmp::Ordering::Greater)
        {
            existing.min = spec.min.clone();
        }
        if !spec.max.is_empty()
            && (existing.max.is_empty()
                || crate::version::compare_to_bound(&spec.max, &existing.max)
                    == std::cmp::Ordering::Less)
        {
            existing.max = spec.max.clone();
        }
        if existing.probe.is_empty() {
            existing.probe = spec.probe.clone();
        }
        // 具名检查取并集，按声明顺序去重。
        for name in &spec.feature_checks {
            if !existing.feature_checks.contains(name) {
                existing.feature_checks.push(name.clone());
            }
        }
        if !existing.min.is_empty()
            && !existing.max.is_empty()
            && crate::version::compare_to_bound(&existing.min, &existing.max)
                == std::cmp::Ordering::Greater
        {
            anyhow::bail!(
                "节点 {node} 的 {} 要求交集为空：min={} 大于 max={}",
                existing.name,
                existing.min,
                existing.max
            );
        }
    }
    Ok(out)
}

struct Resolver<'a, 'o> {
    opts: &'o Options<'a>,
    tools_dir: PathBuf,
    /// 名字 + min + max + check -> 宿主探测结果。
    host: BTreeMap<(String, String, String, String), HostProbe>,
    /// 工具名 -> build/tools 里已安装的版本。
    store: BTreeMap<String, Vec<String>>,
    out: Resolution,
    /// 已经排进 order 的 built 工具 (name, version)。
    seen: BTreeSet<(String, String)>,
}

impl Resolver<'_, '_> {
    fn one(&mut self, spec: &ToolSpec, depth: usize) -> Result<ResolvedTool> {
        if depth > 32 {
            anyhow::bail!("工具依赖成环（{}）", spec.name);
        }
        let name = &spec.name;
        let recipes = self.recipes(name);

        // 1. --lock：state 里记录过的 built 版本优先。
        if self.opts.lock
            && let Some(locked) = self.locked_version(spec)?
        {
            let Some(recipe) = recipes.iter().find(|item| item.version == locked) else {
                anyhow::bail!(
                    "锁定的工具 {name} {locked} 不在 catalog 里；去掉 --lock 或补回 catalog 版本"
                );
            };
            self.install_recipe(recipe, depth)?;
            return Ok(self.built(recipe));
        }

        // 2. 宿主优先：存在 -> 版本 -> feature_checks（有才跑）。
        match self.probe(spec)? {
            HostProbe::Ok(version) => {
                return Ok(ResolvedTool {
                    name: name.clone(),
                    version,
                    provider: Provider::Host,
                    prefix: None,
                });
            }
            HostProbe::Unusable {
                kind,
                packages,
                note,
                message,
            } => return Err(ToolError::boxed(kind, packages, note, message)),
            HostProbe::Absent | HostProbe::OutOfRange(_) => {}
        }

        // 3. build/tools 里已有的版本。
        let installed = self.store_versions(name);
        let reusable = recipes
            .iter()
            .filter(|recipe| installed.iter().any(|v| v == &recipe.version))
            .filter(|recipe| crate::version::in_range(&recipe.version, &spec.min, &spec.max))
            .max_by(|a, b| version_cmp(&a.version, &b.version));
        if let Some(recipe) = reusable {
            self.install_recipe(recipe, depth)?;
            return Ok(self.built(recipe));
        }

        // 4. catalog 里满足区间的最高版本。
        let candidate = recipes
            .iter()
            .filter(|recipe| crate::version::in_range(&recipe.version, &spec.min, &spec.max))
            .max_by(|a, b| version_cmp(&a.version, &b.version));
        if let Some(recipe) = candidate {
            self.install_recipe(recipe, depth)?;
            return Ok(self.built(recipe));
        }

        Err(self.no_candidate(spec, &recipes))
    }

    fn recipes(&self, name: &str) -> Vec<Target> {
        let mut out: Vec<Target> = self
            .opts
            .reg
            .of_kind(Kind::Tool)
            .filter(|target| target.name == name)
            .cloned()
            .collect();
        out.sort_by(|a, b| version_cmp(&a.version, &b.version));
        out
    }

    /// 锁定版本：有记录且满足区间时取最高；记录不满足区间时报错。
    fn locked_version(&self, spec: &ToolSpec) -> Result<Option<String>> {
        let Some(versions) = self.opts.locked.get(&spec.name) else {
            return Ok(None);
        };
        let mut candidates: Vec<&String> = versions
            .iter()
            .filter(|version| crate::version::in_range(version, &spec.min, &spec.max))
            .collect();
        if candidates.is_empty() {
            anyhow::bail!(
                "--lock 记录的工具 {} 版本 [{}] 都不满足当前要求 {}{}；去掉 --lock 重试",
                spec.name,
                versions.join(", "),
                range_text(spec),
                ""
            );
        }
        candidates.sort_by(|a, b| version_cmp(a, b));
        Ok(candidates.last().map(|version| (*version).clone()))
    }

    fn probe(&mut self, spec: &ToolSpec) -> Result<HostProbe> {
        let key = (
            spec.name.clone(),
            spec.min.clone(),
            spec.max.clone(),
            spec.feature_checks.join(","),
        );
        if let Some(value) = self.host.get(&key) {
            return Ok(value.clone());
        }
        let value = self.probe_uncached(spec)?;
        self.host.insert(key, value.clone());
        Ok(value)
    }

    /// 存在 -> 版本 -> feature_checks。检查命令和失败处理都来自工具 recipe。
    fn probe_uncached(&self, spec: &ToolSpec) -> Result<HostProbe> {
        if !on_path(&spec.name) {
            return Ok(HostProbe::Absent);
        }
        let version = if spec.min.is_empty() && spec.max.is_empty() {
            String::new()
        } else {
            let args: Vec<&str> = spec.probe.iter().map(String::as_str).collect();
            let version = probe_program(&spec.name, &args)?;
            if !crate::version::in_range(&version, &spec.min, &spec.max) {
                // 宿主有这个命令但版本不满足：交给 store / catalog。
                return Ok(HostProbe::OutOfRange(version));
            }
            version
        };

        for check_name in &spec.feature_checks {
            let Some(checks) = self.opts.reg.host_tools.get(&spec.name) else {
                anyhow::bail!(
                    "工具 {} 要求检查 {check_name}，但 recipes/tools/{}.star 没有定义任何检查",
                    spec.name,
                    spec.name
                );
            };
            let Some(check) = checks.get(check_name) else {
                anyhow::bail!(
                    "工具 {} 没有定义 feature_check {check_name}（在 recipes/tools/{}.star 里加）",
                    spec.name,
                    spec.name
                );
            };
            let args: Vec<&str> = check.run.iter().map(String::as_str).collect();
            let Err(reason) = run_check(&spec.name, &args) else {
                continue;
            };
            let line = if check.run.is_empty() {
                spec.name.clone()
            } else {
                format!("{} {}", spec.name, check.run.join(" "))
            };
            let (kind, packages, note, message) = match &check.on_fail {
                CheckFail::NeedPackages {
                    names,
                    reason: note,
                } => (
                    UnresolvedKind::MissingCommand,
                    names.clone(),
                    note.clone(),
                    // 写了 reason 就只报这一句，原始输出太长，需要时自己跑检查命令。
                    if note.is_empty() {
                        format!("宿主 {line} 检查不过（{check_name}）：{reason}")
                    } else {
                        note.clone()
                    },
                ),
                CheckFail::ErrorMsg { text } => (
                    UnresolvedKind::Data,
                    Vec::new(),
                    String::new(),
                    format!("{text}\n  {line}: {reason}"),
                ),
            };
            return Ok(HostProbe::Unusable {
                kind,
                packages,
                note,
                message,
            });
        }
        Ok(HostProbe::Ok(version))
    }

    fn store_versions(&mut self, name: &str) -> Vec<String> {
        if let Some(versions) = self.store.get(name) {
            return versions.clone();
        }
        let mut out = Vec::new();
        let prefix = format!("{name}-");
        if let Ok(entries) = std::fs::read_dir(&self.tools_dir) {
            for entry in entries.flatten() {
                let Some(dir) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                let Some(version) = dir.strip_prefix(&prefix) else {
                    continue;
                };
                if crate::recipe::is_version_text(version) && entry.path().is_dir() {
                    out.push(version.to_owned());
                }
            }
        }
        out.sort_by(|a, b| version_cmp(a, b));
        self.store.insert(name.to_owned(), out.clone());
        out
    }

    /// 把工具节点排进 order；它自己的工具先排。
    fn install_recipe(&mut self, recipe: &Target, depth: usize) -> Result<()> {
        let key = (recipe.name.clone(), recipe.version.clone());
        if self.seen.contains(&key) {
            return Ok(());
        }
        let specs = merge(recipe.tools.clone(), &recipe.id())?;
        for spec in &specs {
            match self.one(spec, depth + 1) {
                Ok(tool) => {
                    self.out.per_node.entry(recipe.id()).or_default().push(tool);
                }
                Err(err) if self.opts.lenient => {
                    self.out.unresolved.push(Unresolved {
                        name: spec.name.clone(),
                        reason: format!("{err:#}"),
                        kind: classify(&err),
                        packages: packages_of(&err),
                        note: note_of(&err),
                    });
                }
                Err(err) => return Err(err),
            }
        }
        if self.seen.insert(key) {
            self.out.tool_ids.push(recipe.id());
        }
        Ok(())
    }

    fn built(&self, recipe: &Target) -> ResolvedTool {
        ResolvedTool {
            name: recipe.name.clone(),
            version: recipe.version.clone(),
            provider: Provider::Store,
            prefix: Some(layout::tool_prefix(
                &self.tools_dir,
                &recipe.name,
                &recipe.version,
            )),
        }
    }

    fn no_candidate(&self, spec: &ToolSpec, recipes: &[Target]) -> anyhow::Error {
        let name = &spec.name;
        let range = range_text(spec);
        let key = (
            name.clone(),
            spec.min.clone(),
            spec.max.clone(),
            spec.feature_checks.join(","),
        );
        if let Some(HostProbe::OutOfRange(version)) = self.host.get(&key) {
            return ToolError::boxed(
                UnresolvedKind::OutOfRange,
                Vec::new(),
                String::new(),
                format!("宿主 {name} {version} 不满足 {range}，catalog 也没有可用版本"),
            );
        }
        if recipes.is_empty() {
            return ToolError::boxed(
                UnresolvedKind::MissingCommand,
                Vec::new(),
                String::new(),
                format!("宿主缺少 {name}，catalog 没有 {name} 的构建 recipe"),
            );
        }
        let highest = recipes
            .iter()
            .max_by(|a, b| version_cmp(&a.version, &b.version))
            .map(|recipe| recipe.version.as_str())
            .unwrap_or("");
        ToolError::boxed(
            UnresolvedKind::Data,
            Vec::new(),
            String::new(),
            format!("catalog 里 {name} 最高 {highest}，不满足 {range}"),
        )
    }
}

/// 解析失败要装哪些包；只有 feature_check 的 need_packages 会带。
fn packages_of(err: &anyhow::Error) -> Vec<String> {
    err.downcast_ref::<ToolError>()
        .map(|error| error.packages.clone())
        .unwrap_or_default()
}

/// 工具 recipe 写的说明；没写（或不是工具错误）时为空。
fn note_of(err: &anyhow::Error) -> String {
    err.downcast_ref::<ToolError>()
        .map(|error| error.note.clone())
        .unwrap_or_default()
}

/// 解析失败属于哪一类；没有分类的错误按数据问题处理，保持原错误。
fn classify(err: &anyhow::Error) -> UnresolvedKind {
    err.downcast_ref::<ToolError>()
        .map(|error| error.kind)
        .unwrap_or(UnresolvedKind::Data)
}

fn range_text(spec: &ToolSpec) -> String {
    match (spec.min.is_empty(), spec.max.is_empty()) {
        (false, false) => format!(">= {} <= {}", spec.min, spec.max),
        (false, true) => format!(">= {}", spec.min),
        (true, false) => format!("<= {}", spec.max),
        (true, true) => "任意版本".to_owned(),
    }
}

/// 宿主探测结果。有命令但版本不满足时保留版本，错误文案才能说清是哪种情况。
#[derive(Debug, Clone)]
enum HostProbe {
    Absent,
    OutOfRange(String),
    /// 命令在、版本也行，但 feature_check 没过（如 Debian 的 perl-base 缺 FindBin）。
    Unusable {
        kind: UnresolvedKind,
        /// need_packages 的键；error_msg 时为空。
        packages: Vec<String>,
        /// 工具 recipe 里写的说明；空表示没写。
        note: String,
        message: String,
    },
    Ok(String),
}

/// 可用性检查：退出码 0 才算通过；失败时返回一行原因（stderr 首行）。
fn run_check(program: &str, args: &[&str]) -> std::result::Result<(), String> {
    let output = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|err| format!("跑不起来（{err}）"))?;
    if output.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&output.stderr);
    let first = text.lines().next().unwrap_or("").trim();
    let reason = if first.is_empty() {
        match output.status.code() {
            Some(code) => format!("退出码 {code}"),
            None => "被信号终止".to_owned(),
        }
    } else {
        first.chars().take(160).collect()
    };
    Err(reason)
}

/// 全量版本比较（段数不同时短的补 0）。
pub fn version_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    let left = version_parts(left);
    let right = version_parts(right);
    let len = left.len().max(right.len());
    for index in 0..len {
        let a = *left.get(index).unwrap_or(&0);
        let b = *right.get(index).unwrap_or(&0);
        match a.cmp(&b) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

fn version_parts(text: &str) -> Vec<u64> {
    text.split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse().unwrap_or(0)
        })
        .collect()
}

/// 跑一个命令，返回 stdout + stderr 原文。
pub fn run_program(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .with_context(|| format!("跑不起来 {program}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(text)
}

/// 跑版本探测命令，取输出里的第一个点分数字。
pub fn probe_program(program: &str, args: &[&str]) -> Result<String> {
    let text = run_program(program, args)?;
    find_version(&text).ok_or_else(|| {
        anyhow::anyhow!(
            "拿不到 {program} 版本（命令: {program} {}）",
            args.join(" ")
        )
    })
}

fn find_version(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let begin = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        let candidate = text[begin..index].trim_end_matches('.');
        if candidate.contains('.') {
            return Some(candidate.to_owned());
        }
    }
    None
}

/// 命令是否在 PATH 里且可执行。
pub(crate) fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        candidate.is_file()
            && candidate
                .metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_the_first_dotted_number() {
        assert_eq!(
            find_version("gcc (GCC) 15.3.1 20260722").as_deref(),
            Some("15.3.1")
        );
        assert_eq!(
            find_version("NASM version 2.16.03").as_deref(),
            Some("2.16.03")
        );
        assert_eq!(find_version("v5.38.2").as_deref(), Some("5.38.2"));
        assert_eq!(find_version("no version here"), None);
    }

    fn spec(name: &str, min: &str) -> ToolSpec {
        ToolSpec {
            name: name.to_owned(),
            min: min.to_owned(),
            max: String::new(),
            probe: vec!["--version".to_owned()],
            feature_checks: Vec::new(),
        }
    }

    #[test]
    fn merge_keeps_the_strictest_requirement() {
        let merged = merge(vec![spec("nasm", "2.13"), spec("nasm", "")], "node").unwrap();
        assert_eq!(merged[0].min, "2.13");

        let merged = merge(vec![spec("nasm", ""), spec("nasm", "2.13")], "node").unwrap();
        assert_eq!(merged[0].min, "2.13");

        let merged = merge(vec![spec("gcc", "9"), spec("gcc", "11")], "node").unwrap();
        assert_eq!(merged[0].min, "11");
    }

    #[test]
    fn merge_rejects_an_empty_intersection() {
        let mut low = spec("autoconf", "");
        low.max = "2.69".into();
        let mut high = spec("autoconf", "2.70");
        high.max = String::new();
        assert!(merge(vec![low, high], "openssl").is_err());
    }

    #[test]
    fn check_requires_zero_exit() {
        assert!(run_check("/bin/true", &[]).is_ok());
        assert!(run_check("/bin/false", &[]).is_err());
        assert!(run_check("/nonexistent-phprt-check", &[]).is_err());
    }

    #[test]
    fn feature_checks_are_unioned_across_nodes() {
        let mut a = spec("perl", "5.20");
        a.feature_checks = vec!["findbin".into()];
        let mut b = spec("perl", "5.20");
        b.feature_checks = vec!["findbin".into(), "encode".into()];
        let merged = merge(vec![a, b], "node").unwrap();
        assert_eq!(merged[0].feature_checks, ["findbin", "encode"]);
    }

    #[test]
    fn versions_compare_by_numeric_parts() {
        assert_eq!(version_cmp("15.3.1", "11"), std::cmp::Ordering::Greater);
        assert_eq!(version_cmp("9.4", "11"), std::cmp::Ordering::Less);
        assert_eq!(version_cmp("4.0", "4.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(version_cmp("4.10", "4.9"), std::cmp::Ordering::Greater);
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::*;
    use crate::recipe::{FeatureCheck, Profile};

    fn tool_node(name: &str, version: &str) -> Target {
        Target {
            kind: Kind::Tool,
            name: name.to_owned(),
            version: version.to_owned(),
            url: format!("https://example.com/{name}-{version}.tar.gz"),
            ..Default::default()
        }
    }

    fn product(tools: Vec<ToolSpec>) -> Target {
        Target {
            kind: Kind::Product,
            name: "demo".to_owned(),
            series: "1".to_owned(),
            version: "1.0".to_owned(),
            url: "https://example.com/demo.tar.gz".to_owned(),
            profile: Profile {
                tools,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn spec(min: &str) -> ToolSpec {
        ToolSpec {
            name: "phprt-demo-tool-xyz".to_owned(),
            min: min.to_owned(),
            max: String::new(),
            probe: vec!["--version".to_owned()],
            feature_checks: Vec::new(),
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "phprt-tools-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(layout::tools_dir(&root)).unwrap();
        root
    }

    fn registry(nodes: Vec<Target>) -> Registry {
        Registry::from_targets(nodes).unwrap()
    }

    fn run(
        reg: &Registry,
        product: &Target,
        root: &Path,
        locked: BTreeMap<String, Vec<String>>,
        lock: bool,
    ) -> Result<Resolution> {
        let sel = Selection::default();
        resolve(
            &[product],
            &Options {
                reg,
                product,
                sel: &sel,
                build_root: root,
                locked,
                lock,
                lenient: false,
            },
        )
    }

    #[test]
    fn store_reuse_beats_catalog() {
        let root = temp_root("store");
        std::fs::create_dir_all(layout::tools_dir(&root).join("phprt-demo-tool-xyz-1.2")).unwrap();
        let reg = registry(vec![
            tool_node("phprt-demo-tool-xyz", "1.2"),
            tool_node("phprt-demo-tool-xyz", "2.0"),
        ]);
        let product = product(vec![spec("1.0")]);
        let out = run(&reg, &product, &root, BTreeMap::new(), false).unwrap();
        let tools = out.tools_of(&product.id());
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].provider, Provider::Store);
        assert_eq!(tools[0].version, "1.2");
        assert_eq!(out.tool_ids, vec!["tool:phprt-demo-tool-xyz@1.2"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn catalog_picks_the_highest_satisfying_version() {
        let root = temp_root("catalog");
        let reg = registry(vec![
            tool_node("phprt-demo-tool-xyz", "1.2"),
            tool_node("phprt-demo-tool-xyz", "2.0"),
        ]);
        let product = product(vec![spec("1.0")]);
        let out = run(&reg, &product, &root, BTreeMap::new(), false).unwrap();
        assert_eq!(out.tools_of(&product.id())[0].version, "2.0");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn lock_prefers_the_recorded_built_version() {
        let root = temp_root("lock");
        std::fs::create_dir_all(layout::tools_dir(&root).join("phprt-demo-tool-xyz-1.2")).unwrap();
        std::fs::create_dir_all(layout::tools_dir(&root).join("phprt-demo-tool-xyz-1.5")).unwrap();
        let reg = registry(vec![
            tool_node("phprt-demo-tool-xyz", "1.2"),
            tool_node("phprt-demo-tool-xyz", "1.5"),
        ]);
        let product = product(vec![spec("1.0")]);
        let mut locked = BTreeMap::new();
        locked.insert("phprt-demo-tool-xyz".to_owned(), vec!["1.2".to_owned()]);
        let out = run(&reg, &product, &root, locked, true).unwrap();
        assert_eq!(out.tools_of(&product.id())[0].version, "1.2");
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn dep_node(name: &str, tools: Vec<ToolSpec>) -> Target {
        Target {
            kind: Kind::Dep,
            name: name.to_owned(),
            version: "1".to_owned(),
            url: format!("https://example.com/{name}.tar.gz"),
            tools,
            ..Default::default()
        }
    }

    #[test]
    fn the_same_tool_can_resolve_to_different_versions_per_node() {
        let root = temp_root("multi");
        let reg = registry(vec![
            tool_node("phprt-demo-tool-xyz", "2.71"),
            tool_node("phprt-demo-tool-xyz", "2.72"),
        ]);
        let product = product(vec![]);
        let mut older = spec("2.68");
        older.max = "2.71".to_owned();
        let old_node = dep_node("old", vec![older]);
        let new_node = dep_node("new", vec![spec("2.70")]);
        let sel = Selection::default();
        let order = [&product, &old_node, &new_node];
        let out = resolve(
            &order,
            &Options {
                reg: &reg,
                product: &product,
                sel: &sel,
                build_root: &root,
                locked: BTreeMap::new(),
                lock: false,
                lenient: false,
            },
        )
        .unwrap();
        assert_eq!(out.tools_of(&old_node.id())[0].version, "2.71");
        assert_eq!(out.tools_of(&new_node.id())[0].version, "2.72");
        assert_eq!(
            out.tool_ids,
            vec![
                "tool:phprt-demo-tool-xyz@2.71",
                "tool:phprt-demo-tool-xyz@2.72"
            ]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn real_spec(name: &str) -> ToolSpec {
        // 用真实存在的命令，检查才是唯一变量。
        ToolSpec {
            name: name.to_owned(),
            min: String::new(),
            max: String::new(),
            probe: vec!["--version".to_owned()],
            feature_checks: Vec::new(),
        }
    }

    #[test]
    fn undefined_feature_check_stops_resolution() {
        let root = temp_root("no-check");
        let reg = registry(vec![]);
        let mut spec = real_spec("true");
        spec.feature_checks = vec!["findbin".into()];
        let product = product(vec![spec]);
        let err = run(&reg, &product, &root, BTreeMap::new(), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("没有定义任何检查"), "{err}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn failed_check_with_need_packages_becomes_missing_command() {
        let root = temp_root("need-packages");
        let mut reg = registry(vec![]);
        reg.host_tools.insert(
            "false".into(),
            BTreeMap::from([(
                "findbin".into(),
                FeatureCheck {
                    run: vec!["-e".into(), "1".into()],
                    on_fail: CheckFail::NeedPackages {
                        names: vec!["perl".into()],
                        reason: "当前 perl 模块不全".into(),
                    },
                },
            )]),
        );
        let mut spec = real_spec("false");
        spec.feature_checks = vec!["findbin".into()];
        let product = product(vec![spec]);
        let sel = Selection::default();
        let out = resolve(
            &[&product],
            &Options {
                reg: &reg,
                product: &product,
                sel: &sel,
                build_root: &root,
                locked: BTreeMap::new(),
                lock: false,
                lenient: true,
            },
        )
        .unwrap();
        assert_eq!(out.unresolved.len(), 1);
        assert_eq!(out.unresolved[0].kind, UnresolvedKind::MissingCommand);
        assert_eq!(out.unresolved[0].packages, ["perl"]);
        // 写了 reason 就只报这一句，不再带一长串原始输出。
        assert_eq!(out.unresolved[0].note, "当前 perl 模块不全");
        assert_eq!(out.unresolved[0].reason, "当前 perl 模块不全");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn failed_check_with_error_msg_is_not_installable() {
        let root = temp_root("error-msg");
        let mut reg = registry(vec![]);
        reg.host_tools.insert(
            "false".into(),
            BTreeMap::from([(
                "findbin".into(),
                FeatureCheck {
                    run: vec!["-e".into(), "1".into()],
                    on_fail: CheckFail::ErrorMsg {
                        text: "perl 是定制构建的，装包无效".into(),
                    },
                },
            )]),
        );
        let mut spec = real_spec("false");
        spec.feature_checks = vec!["findbin".into()];
        let product = product(vec![spec]);
        let sel = Selection::default();
        let out = resolve(
            &[&product],
            &Options {
                reg: &reg,
                product: &product,
                sel: &sel,
                build_root: &root,
                locked: BTreeMap::new(),
                lock: false,
                lenient: true,
            },
        )
        .unwrap();
        assert_eq!(out.unresolved.len(), 1);
        assert_eq!(out.unresolved[0].kind, UnresolvedKind::Data);
        assert!(out.unresolved[0].packages.is_empty());
        assert!(
            out.unresolved[0].reason.contains("装包无效"),
            "{}",
            out.unresolved[0].reason
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn missing_tool_names_the_reason() {
        let root = temp_root("missing");
        let reg = registry(vec![tool_node("phprt-demo-tool-xyz", "1.2")]);
        let product = product(vec![spec("9.0")]);
        let err = run(&reg, &product, &root, BTreeMap::new(), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("最高 1.2，不满足 >= 9.0"), "{err}");

        let reg = registry(vec![]);
        let err = run(&reg, &product, &root, BTreeMap::new(), false)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("没有 phprt-demo-tool-xyz 的构建 recipe"),
            "{err}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
