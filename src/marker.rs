//! 安装完成标记 `.phprt/<产品>-<版本>/.phprt-installed`。
//!
//! 安装树目录存在只说明“试过”，标记存在才说明“装完”。标记在整条流水线
//! （下载、编译、收库、ini、产品校验、写入口）成功后最后一步原子写入，
//! 内容是补丁号和写标记时的方案指纹。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::recipe::{Selection, Target, TargetSelection};

pub const FILE_NAME: &str = ".phprt-installed";

/// 安装树在磁盘上的样子。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Site {
    /// 目录和标记都没有。
    Absent,
    /// 目录在、标记不在：上次失败或中断，可以继续。
    Interrupted,
    /// 标记在，内容就是这棵树装的到底是什么。
    Complete(InstalledTree),
}

/// 一棵安装树里某个组件留下的东西：删组件时按它删文件、核对 ini 有没有被改。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComponentFacts {
    pub name: String,
    pub mode: String,
    /// ini 相对安装树；空表示这棵树里没有它的登记文件（如 fold 组件）。
    pub ini: String,
    /// ini 内容指纹；ini 为空时为空。
    pub ini_hash: String,
    /// 产物相对安装树。
    pub artifacts: Vec<String>,
}

/// 完成标记的内容：补丁号、方案指纹、每个组件的产物和 ini。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstalledTree {
    pub version: String,
    /// 方案指纹，用来快速判断“配置一致不一致”。
    pub scheme: String,
    /// 本次实际解析出的版本指纹；只用于比较，不参与解析。
    pub resolution: String,
    /// 当时那条可复制的 install 命令，用来给用户看差异。
    pub scheme_cmd: String,
    pub components: Vec<ComponentFacts>,
}

impl InstalledTree {
    pub fn component(&self, name: &str) -> Option<&ComponentFacts> {
        self.components.iter().find(|item| item.name == name)
    }
}

/// install 的判定结果：直接装、先备份旧树再装，或者接着装没装完的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Install,
    InstallAfterBackup,
    Continue,
}

/// `install` 之前看到的现场。全是判定需要的输入，不看磁盘状态以外的东西。
pub struct InstallFacts<'a> {
    pub product: &'a str,
    pub series: &'a str,
    /// 本次要装的补丁。
    pub target: &'a str,
    /// state 记录的已装补丁；空表示没写过状态。
    pub installed: &'a str,
    /// 用户写的是补丁号（`php@8.5.12`），不是系列（`php@8.5`）。
    pub explicit_patch: bool,
    /// 目标补丁的安装树。
    pub site: Site,
    /// 当前方案指纹。
    pub scheme: &'a str,
    /// 本次实际解析出的版本指纹。
    pub resolution: &'a str,
    /// 本次方案的可执行命令，配置不一致时打给用户对比。
    pub requested_command: &'a str,
    /// 本次目标安装树的显示路径（全局模式是绝对路径）。
    pub tree: String,
    /// 已装版本安装树的显示路径。
    pub installed_tree: String,
    /// 建议命令的前缀，如 `phprt` 或 `phprt --global --prefix /usr/local/php`。
    pub command: &'a str,
    /// 本次的 `--targets '...'` 片段（含前导空格）；空表示产品没有 targets。
    pub target_args: &'a str,
}

pub fn tree_path(root: &Path, product: &str, version: &str) -> PathBuf {
    crate::layout::tree_path(root, product, version)
}

pub fn marker_path(root: &Path, product: &str, version: &str) -> PathBuf {
    tree_path(root, product, version).join(FILE_NAME)
}

/// 读安装树现状。标记内容读不懂时按“没有标记”处理：宁可多装一次，
/// 也不把半截的树当成装完。
pub fn site(root: &Path, product: &str, version: &str) -> Site {
    let tree = tree_path(root, product, version);
    if let Some(installed) = read_marker(&tree.join(FILE_NAME))
        && installed.version == version
    {
        return Site::Complete(installed);
    }
    // 标记里的补丁和期望补丁不一致（目录被改名、全局 prefix 换了补丁）
    // 按“没有标记”处理：宁可多装一次，也不把错的树当成装完。
    if tree.is_dir() {
        Site::Interrupted
    } else {
        Site::Absent
    }
}

fn read_marker(path: &Path) -> Option<InstalledTree> {
    let text = fs::read_to_string(path).ok()?;
    let mut tree = InstalledTree::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("version=") {
            tree.version = value.trim().to_owned();
        } else if let Some(value) = line.strip_prefix("scheme=") {
            tree.scheme = value.trim().to_owned();
        } else if let Some(value) = line.strip_prefix("resolution=") {
            tree.resolution = value.trim().to_owned();
        } else if let Some(value) = line.strip_prefix("scheme_cmd=") {
            tree.scheme_cmd = value.trim().to_owned();
        } else if let Some(value) = line.strip_prefix("component=") {
            let (name, mode) = value.split_once(" mode=").unwrap_or((value, ""));
            tree.components.push(ComponentFacts {
                name: name.trim().to_owned(),
                mode: mode.trim().to_owned(),
                ..ComponentFacts::default()
            });
        } else if let Some(value) = line.strip_prefix("ini=") {
            if let Some((name, path)) = value.split_once(' ')
                && let Some(item) = tree.components.iter_mut().find(|item| item.name == name)
            {
                item.ini = path.trim().to_owned();
            }
        } else if let Some(value) = line.strip_prefix("ini_hash=") {
            if let Some((name, hash)) = value.split_once(' ')
                && let Some(item) = tree.components.iter_mut().find(|item| item.name == name)
            {
                item.ini_hash = hash.trim().to_owned();
            }
        } else if let Some(value) = line.strip_prefix("artifact=")
            && let Some((name, path)) = value.split_once(' ')
            && let Some(item) = tree.components.iter_mut().find(|item| item.name == name)
        {
            item.artifacts.push(path.trim().to_owned());
        }
    }
    if tree.version.is_empty() || tree.scheme.is_empty() {
        return None;
    }
    Some(tree)
}

/// 最后一步调用的写标记：先写同目录临时文件再改名，进程被杀不会留下半个标记。
///
/// 补丁号显式传入，不从内容反推路径；内容里的 `version=` 必须和它一致。
pub fn write(root: &Path, product: &str, version: &str, tree: &InstalledTree) -> Result<()> {
    let path = marker_path(root, product, version);
    let dir = path
        .parent()
        .with_context(|| format!("算不出 {} 的目录", path.display()))?;
    fs::create_dir_all(dir).with_context(|| format!("建不了 {}", dir.display()))?;

    let mut text = format!("version={version}\nscheme={}\n", tree.scheme);
    if !tree.resolution.is_empty() {
        text.push_str(&format!("resolution={}\n", tree.resolution));
    }
    if !tree.scheme_cmd.is_empty() {
        text.push_str(&format!("scheme_cmd={}\n", tree.scheme_cmd));
    }
    for item in &tree.components {
        text.push_str(&format!("component={} mode={}\n", item.name, item.mode));
        if !item.ini.is_empty() {
            text.push_str(&format!("ini={} {}\n", item.name, item.ini));
        }
        if !item.ini_hash.is_empty() {
            text.push_str(&format!("ini_hash={} {}\n", item.name, item.ini_hash));
        }
        for artifact in &item.artifacts {
            text.push_str(&format!("artifact={} {}\n", item.name, artifact));
        }
    }

    let tmp = dir.join(format!("{FILE_NAME}.tmp"));
    fs::write(&tmp, text).with_context(|| format!("写不了 {}", tmp.display()))?;
    fs::rename(&tmp, &path)
        .with_context(|| format!("改名不了 {} → {}", tmp.display(), path.display()))?;
    Ok(())
}

/// 一次构建的方案：指纹 + 可复制的 install 命令。
pub struct Scheme {
    pub digest: String,
    pub command: String,
}

impl Scheme {
    pub fn new(
        product: &str,
        series: &str,
        sel: &Selection,
        bindings: &BTreeMap<String, String>,
        targets: &TargetSelection,
        is_dep: &dyn Fn(&str) -> bool,
    ) -> Self {
        Self {
            digest: scheme_digest(sel, bindings, targets),
            command: scheme_command(product, series, sel, bindings, targets, is_dep),
        }
    }
}

/// 把方案渲染成一条可复制执行的 install 命令。
///
/// 含：完整组件列表、mode、组件自己的 feature、依赖转授、用户钉的版本线；
/// 不含：补丁号、解析出来的依赖线交集、feature 并集、configure 参数。
pub fn scheme_command(
    product: &str,
    series: &str,
    sel: &Selection,
    bindings: &BTreeMap<String, String>,
    targets: &TargetSelection,
    is_dep: &dyn Fn(&str) -> bool,
) -> String {
    render_scheme(product, series, None, sel, bindings, targets, None, is_dep)
}

/// `scheme --lock`：和普通 `scheme` 共用渲染，只额外写产品补丁和精确版本。
#[allow(clippy::too_many_arguments)]
pub fn scheme_command_locked(
    product: &str,
    series: &str,
    version: &str,
    sel: &Selection,
    bindings: &BTreeMap<String, String>,
    targets: &TargetSelection,
    exact: &BTreeMap<String, String>,
    is_dep: &dyn Fn(&str) -> bool,
) -> String {
    render_scheme(
        product,
        series,
        Some(version),
        sel,
        bindings,
        targets,
        Some(exact),
        is_dep,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_scheme(
    product: &str,
    series: &str,
    version: Option<&str>,
    sel: &Selection,
    bindings: &BTreeMap<String, String>,
    targets: &TargetSelection,
    exact: Option<&BTreeMap<String, String>>,
    is_dep: &dyn Fn(&str) -> bool,
) -> String {
    let mut specs = Vec::new();
    for (name, spec) in &sel.per_component {
        let mut text = name.clone();
        if let Some(mode) = &spec.mode
            && !mode.is_empty()
        {
            text.push_str(&format!("=mode:{mode}"));
        }
        if let Some(features) = &spec.features
            && !features.is_empty()
        {
            let list: Vec<&str> = features.iter().map(String::as_str).collect();
            text.push_str(&format!(",feature:[{}]", list.join(",")));
        }
        for (dep, features) in &spec.deps {
            if features.is_empty() {
                text.push_str(&format!(",dep:{dep}"));
            } else {
                text.push_str(&format!(",dep:{dep}[{}]", features.join(",")));
            }
        }
        let pinned = exact
            .and_then(|exact| exact.get(name))
            .filter(|_| !is_dep(name))
            .cloned();
        let line = pinned
            .or_else(|| spec.version.clone())
            .or_else(|| bindings.get(name).filter(|_| !is_dep(name)).cloned());
        if let Some(line) = line {
            text.push_str(&format!(",version:{line}"));
        }
        specs.push(text);
    }
    let mut pins = String::new();
    if let Some(exact) = exact {
        for (name, version) in exact {
            if is_dep(name) {
                pins.push_str(&format!(" --dep {name}={version}"));
            }
        }
    }
    for (name, line) in bindings {
        let already_exact = exact.is_some_and(|exact| exact.contains_key(name));
        if is_dep(name) && !already_exact {
            pins.push_str(&format!(" --dep {name}={line}"));
        }
    }
    let target = match version {
        Some(version) => format!("{product}@{version}"),
        None => format!("{product}@{series}"),
    };
    format!(
        "phprt install {target} '{}'{}{pins}",
        specs.join(","),
        crate::targets::scheme_args(targets)
    )
}

/// 方案指纹：只含用户输入——组件名、mode、feature、依赖转授、版本线、绑定。
/// 不含补丁号、解析出的依赖线交集、configure 参数、环境变量。
pub fn scheme_digest(
    sel: &Selection,
    bindings: &BTreeMap<String, String>,
    targets: &TargetSelection,
) -> String {
    let mut text = String::new();
    text.push_str(&crate::targets::digest_text(targets));
    for (name, spec) in &sel.per_component {
        text.push_str(&format!("component={name}\n"));
        text.push_str(&format!("mode={}\n", spec.mode.as_deref().unwrap_or("")));
        match &spec.features {
            Some(features) => {
                text.push_str("features=on\n");
                for feature in features {
                    text.push_str(&format!("feature={feature}\n"));
                }
            }
            None => text.push_str("features=default\n"),
        }
        for (dep, features) in &spec.deps {
            let mut features: Vec<&String> = features.iter().collect();
            features.sort();
            let joined: Vec<&str> = features.iter().map(|item| item.as_str()).collect();
            text.push_str(&format!("dep={dep}[{}]\n", joined.join(",")));
        }
        if let Some(line) = &spec.version {
            text.push_str(&format!("version={line}\n"));
        }
    }
    for (name, line) in bindings {
        text.push_str(&format!("binding={name}={line}\n"));
    }
    digest(&text)
}

/// 文件内容指纹，用来判断 ini 有没有被手改过。
pub fn file_digest(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("读不了 {}", path.display()))?;
    Ok(digest(&String::from_utf8_lossy(&bytes)))
}

/// 一次解析的版本指纹：产品补丁 + 每个构建节点的 line / version / sha256。
///
/// 结果与节点传入顺序无关：先按 `Target::id` 建 BTreeMap，再按固定字段顺序
/// 序列化，最后算 sha256。同一组节点无论以什么顺序收集，指纹都相同。
///
/// 只用于比较“这次解析和上次是不是同一批补丁”，不能拿来还原版本。
pub fn resolution_digest(product_version: &str, nodes: &[&Target]) -> String {
    let mut canonical: BTreeMap<String, String> = BTreeMap::new();
    for node in nodes {
        canonical.insert(
            node.id(),
            format!(
                "kind={}\nname={}\nline={}\nversion={}\nsha256={}\n",
                node.kind.as_str(),
                node.name,
                node.line,
                node.version,
                node.sha256
            ),
        );
    }
    let mut text = format!("product={product_version}\n");
    for (id, body) in canonical {
        text.push_str(&format!("node={id}\n{body}"));
    }
    digest(&text)
}

fn digest(text: &str) -> String {
    use sha2::Digest;
    let value = sha2::Sha256::digest(text.as_bytes());
    value.iter().map(|b| format!("{b:02x}")).collect()
}

/// `install` 的现场判定。`--force` 只跳过这里的拒绝，不影响后面的下载和编译错误。
pub fn decide(facts: &InstallFacts<'_>, force: bool) -> Result<Plan> {
    let InstallFacts {
        product,
        series,
        target,
        installed,
        explicit_patch,
        site,
        scheme,
        resolution,
        requested_command,
        tree,
        installed_tree,
        command,
        target_args,
    } = facts;

    if !installed.is_empty() && installed == target {
        return match (site, force) {
            // state 说装过，树被删了：没有什么要保护的东西，直接装。
            (Site::Absent, _) => Ok(Plan::Install),
            // 目标补丁已经装过：先把现有树改名备份，再全新安装。
            (_, true) => Ok(Plan::InstallAfterBackup),
            // 目录在、标记不在：上次没装完。
            (Site::Interrupted, false) => Ok(Plan::Continue),
            (Site::Complete(record), false) => {
                let mut head =
                    format!("{product} {target} 已安装（{tree}），编译配置与本次方案一致");
                if record.scheme != *scheme {
                    head = format!("{product} {target} 已安装（{tree}），编译配置与本次方案不一致");
                    if !record.scheme_cmd.is_empty() {
                        head.push_str(&format!(
                            "\n  已装方案: {}\n  本次方案: {}",
                            record.scheme_cmd, requested_command
                        ));
                    } else {
                        head.push_str(&format!(
                            "\n  本次方案: {requested_command}\n  已装方案没记录，用 phprt scheme {product}@{series} 查看"
                        ));
                    }
                } else if !record.resolution.is_empty() && record.resolution != *resolution {
                    head = format!(
                        "{product} {target} 已安装（{tree}），编译配置一致，但解析出的版本与本次不同"
                    );
                }
                bail!(
                    "{head}\n  \
                     要重新安装，任选其一：\n    \
                     {command} install {product}@{target}{target_args} --force     # 已装版本改名成 {tree}-backup-<日期>，然后全新安装\n    \
                     rm -rf {tree}             # 确认不再需要这个版本，删掉后再装"
                )
            }
        };
    }

    if force {
        // 补丁不同：直接装目标补丁，不动已装版本的安装树。
        return Ok(Plan::Install);
    }
    if *explicit_patch {
        return Ok(Plan::Install);
    }
    if matches!(site, Site::Interrupted) {
        // 目标补丁的树是半成品：接着装。
        return Ok(Plan::Continue);
    }
    if installed.is_empty() {
        return Ok(Plan::Install);
    }

    bail!(
        "已存在旧版本 {product} {installed}（{installed_tree}），本次解析到 {target}\n  \
         要装 {target}，请显式写补丁号：\n    \
         {command} install {product}@{target}{target_args}\n  \
         或者交给 --force：直接装 {target}，不动 {installed} 这个版本\n    \
         {command} install {product}@{series}{target_args} --force"
    )
}

/// 把一棵安装树改名成 `<产品>-<补丁>-backup-<日期>`；重名依次加 `-2`、`-3`。
pub fn backup_tree(root: &Path, product: &str, version: &str) -> Result<PathBuf> {
    let from = tree_path(root, product, version);
    if !from.is_dir() {
        bail!("{} 不存在", from.display());
    }
    let parent = from
        .parent()
        .with_context(|| format!("算不出 {} 的目录", from.display()))?;
    let stamp = stamp()?;
    let base = if crate::layout::is_global() {
        let name = from
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{product}-{version}"));
        format!("{name}-backup-{stamp}")
    } else {
        format!("{product}-{version}-backup-{stamp}")
    };
    let mut target = parent.join(&base);
    let mut index = 1;
    while target.exists() {
        index += 1;
        target = parent.join(format!("{base}-{index}"));
    }
    fs::rename(&from, &target)
        .with_context(|| format!("改名不了 {} → {}", from.display(), target.display()))?;
    Ok(target)
}

/// 备份目录名里的日期，用宿主 `date`，和文档里的 `$(date +%F-%H%M)` 一致。
fn stamp() -> Result<String> {
    let output = Command::new("date")
        .arg("+%F-%H%M")
        .output()
        .context("跑不起来 date")?;
    if !output.status.success() {
        bail!("date 退出 {}，拿不到当前日期", output.status);
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if text.is_empty() {
        bail!("date 没有输出，拿不到当前日期");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{ComponentSpec, Selection};

    fn facts<'a>(site: Site, scheme: &'a str) -> InstallFacts<'a> {
        InstallFacts {
            product: "php",
            series: "8.4",
            target: "8.4.14",
            installed: "8.4.14",
            explicit_patch: true,
            site,
            scheme,
            resolution: "res",
            requested_command: "phprt install php@8.4 'swoole=mode=shared'",
            tree: ".phprt/php-8.4.14".into(),
            installed_tree: ".phprt/php-8.4.14".into(),
            command: "phprt",
            target_args: "",
        }
    }

    fn complete(scheme: &str) -> Site {
        Site::Complete(InstalledTree {
            version: "8.4.14".into(),
            scheme: scheme.to_owned(),
            resolution: String::new(),
            scheme_cmd: "phprt install php@8.4 'gd=mode:static'".to_string(),
            components: Vec::new(),
        })
    }

    #[test]
    fn duplicate_install_says_config_matches() {
        let err = decide(&facts(complete("abc"), "abc"), false)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("php 8.4.14 已安装（.phprt/php-8.4.14），编译配置与本次方案一致"),
            "{err}"
        );
        assert!(err.contains("phprt install php@8.4.14 --force"), "{err}");
        assert!(err.contains("rm -rf .phprt/php-8.4.14"), "{err}");
        assert!(!err.contains("update"), "不该再提示 update：{err}");
    }

    #[test]
    fn duplicate_install_says_config_differs() {
        let err = decide(&facts(complete("old"), "new"), false)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("编译配置与本次方案不一致"),
            "方案不同要拒绝并说明不一致：{err}"
        );
        assert!(!err.contains("编译配置与本次方案一致"), "{err}");
        // 两份方案都打出来，用户能直接对比。
        assert!(
            err.contains("已装方案: phprt install php@8.4 'gd=mode:static'"),
            "{err}"
        );
        assert!(
            err.contains("本次方案: phprt install php@8.4 'swoole=mode=shared'"),
            "{err}"
        );
    }

    #[test]
    fn force_on_installed_patch_backs_up_then_installs() {
        assert_eq!(
            decide(&facts(complete("abc"), "abc"), true).unwrap(),
            Plan::InstallAfterBackup
        );
        // 半成品树也可以先改名再全新装。
        assert_eq!(
            decide(&facts(Site::Interrupted, "abc"), true).unwrap(),
            Plan::InstallAfterBackup
        );
    }

    #[test]
    fn unfinished_install_continues() {
        assert_eq!(
            decide(&facts(Site::Interrupted, "abc"), false).unwrap(),
            Plan::Continue
        );
    }

    #[test]
    fn deleted_tree_installs_directly() {
        assert_eq!(
            decide(&facts(Site::Absent, "abc"), false).unwrap(),
            Plan::Install
        );
    }

    #[test]
    fn series_install_over_older_patch_asks_for_explicit_patch() {
        let mut facts = facts(Site::Absent, "abc");
        facts.installed = "8.4.12";
        facts.installed_tree = ".phprt/php-8.4.12".into();
        facts.explicit_patch = false;
        let err = decide(&facts, false).unwrap_err().to_string();
        assert!(
            err.contains("已存在旧版本 php 8.4.12（.phprt/php-8.4.12），本次解析到 8.4.14"),
            "{err}"
        );
        assert!(err.contains("phprt install php@8.4.14"), "{err}");
        assert!(!err.contains("catalog"), "不要说 catalog 是不是最新：{err}");
    }

    #[test]
    fn force_over_older_patch_installs_without_backup() {
        let mut facts = facts(Site::Absent, "abc");
        facts.installed = "8.4.12";
        facts.explicit_patch = false;
        assert_eq!(decide(&facts, true).unwrap(), Plan::Install);
    }

    #[test]
    fn explicit_patch_over_older_patch_installs() {
        let mut facts = facts(Site::Absent, "abc");
        facts.installed = "8.4.12";
        facts.explicit_patch = true;
        assert_eq!(decide(&facts, false).unwrap(), Plan::Install);
    }

    #[test]
    fn first_install_is_always_allowed() {
        let mut facts = facts(Site::Absent, "abc");
        facts.installed = "";
        assert_eq!(decide(&facts, false).unwrap(), Plan::Install);
    }

    #[test]
    fn backup_tree_renames_and_avoids_collision() {
        let root = std::env::temp_dir().join(format!("phprt-backup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let tree = tree_path(&root, "php", "8.4.14");
        fs::create_dir_all(&tree).unwrap();
        fs::write(tree.join(FILE_NAME), "version=8.4.14\nscheme=abc\n").unwrap();

        let first = backup_tree(&root, "php", "8.4.14").unwrap();
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("php-8.4.14-backup-"), "{name}");
        assert!(!tree.exists(), "原树应该已经改名");
        assert!(first.join(FILE_NAME).is_file(), "备份里要保留标记");

        // 同一分钟再备份一次：加 -2。
        fs::create_dir_all(&tree).unwrap();
        let second = backup_tree(&root, "php", "8.4.14").unwrap();
        let name = second.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with("-2"), "{name}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backup_tree_reports_missing_tree() {
        let root = std::env::temp_dir().join(format!("phprt-backup-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let err = backup_tree(&root, "php", "8.4.14").unwrap_err().to_string();
        assert!(err.contains("不存在"), "{err}");
    }

    fn node(kind: &str, name: &str, version: &str, sha256: &str) -> Target {
        serde_json::from_value(serde_json::json!({
            "kind": kind,
            "name": name,
            "version": version,
            "sha256": sha256,
        }))
        .unwrap()
    }

    #[test]
    fn resolution_digest_ignores_node_order() {
        let php = node("product", "php", "8.5.12", "aaa");
        let openssl = node("dep", "openssl", "3.5.9", "bbb");
        let zlib = node("dep", "zlib", "1.3.2", "ccc");

        let forward = resolution_digest("8.5.12", &[&php, &openssl, &zlib]);
        let backward = resolution_digest("8.5.12", &[&zlib, &php, &openssl]);
        assert_eq!(forward, backward);

        let newer = resolution_digest(
            "8.5.12",
            &[&php, &node("dep", "openssl", "3.5.10", "bbb"), &zlib],
        );
        assert_ne!(forward, newer);
    }

    #[test]
    fn marker_version_must_match_the_expected_tree() {
        let root = std::env::temp_dir().join(format!("phprt-marker-ver-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = tree_path(&root, "php", "8.5.13");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(FILE_NAME),
            "version=8.5.12\nscheme=abc\nresolution=res\n",
        )
        .unwrap();
        // 目录被改名或 prefix 换了补丁：标记不能证明这棵树是 8.5.13。
        assert_eq!(site(&root, "php", "8.5.13"), Site::Interrupted);
        assert_eq!(site(&root, "php", "8.5.12"), Site::Absent);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn same_scheme_different_resolution_is_not_consistent() {
        let mut site = complete("abc");
        if let Site::Complete(tree) = &mut site {
            tree.resolution = "old".into();
        }
        let err = decide(&facts(site, "abc"), false).unwrap_err().to_string();
        assert!(
            err.contains("编译配置一致，但解析出的版本与本次不同"),
            "{err}"
        );
    }

    #[test]
    fn marker_roundtrip() {
        let root = std::env::temp_dir().join(format!("phprt-marker-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert_eq!(site(&root, "php", "8.5.12"), Site::Absent);

        fs::create_dir_all(tree_path(&root, "php", "8.5.12")).unwrap();
        assert_eq!(site(&root, "php", "8.5.12"), Site::Interrupted);

        let tree = InstalledTree {
            version: "8.5.12".into(),
            scheme: "abc123".into(),
            resolution: "res123".into(),
            scheme_cmd: "phprt install php@8.5 'swoole=mode:shared'".into(),
            components: vec![ComponentFacts {
                name: "swoole".into(),
                mode: "shared".into(),
                ini: "etc/conf.d/27-swoole.ini".into(),
                ini_hash: "deadbeef".into(),
                artifacts: vec!["lib/php/extensions/x/swoole.so".into()],
            }],
        };
        write(&root, "php", "8.5.12", &tree).unwrap();
        assert_eq!(site(&root, "php", "8.5.12"), Site::Complete(tree.clone()));
        assert_eq!(
            tree.component("swoole").map(|item| item.ini.as_str()),
            Some("etc/conf.d/27-swoole.ini")
        );
        assert_eq!(
            marker_path(&root, "php", "8.5.12"),
            root.join(".phprt/php-8.5.12/.phprt-installed")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_marker_counts_as_interrupted() {
        let root = std::env::temp_dir().join(format!("phprt-marker-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let tree = tree_path(&root, "php", "8.5.12");
        fs::create_dir_all(&tree).unwrap();
        fs::write(tree.join(FILE_NAME), "{}").unwrap();
        assert_eq!(site(&root, "php", "8.5.12"), Site::Interrupted);
        let _ = fs::remove_dir_all(&root);
    }

    fn spec(mode: Option<&str>, features: Option<&[&str]>, line: Option<&str>) -> ComponentSpec {
        ComponentSpec {
            mode: mode.map(str::to_owned),
            features: features.map(|list| list.iter().map(|item| item.to_string()).collect()),
            version: line.map(str::to_owned),
            ..ComponentSpec::default()
        }
    }

    fn targets(spec: &str) -> TargetSelection {
        crate::targets::parse_cli(&[spec.to_owned()]).unwrap()
    }

    #[test]
    fn scheme_digest_and_command_include_targets() {
        let sel = Selection::default();
        let empty = BTreeMap::new();
        let a = targets("cli,fpm=user:www-data,group:www-data");
        let b = targets("fpm=group:www-data,user:www-data,cli");
        assert_eq!(
            scheme_digest(&sel, &empty, &a),
            scheme_digest(&sel, &empty, &b),
            "target / 配置顺序不应改变指纹"
        );
        assert_ne!(
            scheme_digest(&sel, &empty, &a),
            scheme_digest(&sel, &empty, &TargetSelection::new()),
            "targets 变化必须改变指纹"
        );
        let command = scheme_command("php", "8.5", &sel, &empty, &a, &|_| false);
        assert!(
            command.contains("--targets 'cli,fpm=group:www-data,user:www-data'"),
            "{command}"
        );
    }

    #[test]
    fn scheme_digest_follows_user_input_only() {
        let mut sel = Selection::default();
        sel.per_component
            .insert("gd".into(), spec(Some("static"), None, None));
        let empty = BTreeMap::new();
        let targets = TargetSelection::new();
        let base = scheme_digest(&sel, &empty, &targets);

        // 同样的输入必然得到同样的指纹。
        assert_eq!(base, scheme_digest(&sel, &empty, &targets));

        // mode / feature / 版本线 / 绑定 任一变化都会变。
        for changed in [
            spec(Some("shared"), None, None),
            spec(Some("static"), Some(&["jpeg"]), None),
            spec(Some("static"), None, Some("3.5")),
        ] {
            let mut other = Selection::default();
            other.per_component.insert("gd".into(), changed);
            assert_ne!(base, scheme_digest(&other, &empty, &targets));
        }
        let bindings = BTreeMap::from([("openssl".to_string(), "3.5".to_string())]);
        assert_ne!(base, scheme_digest(&sel, &bindings, &targets));

        // 依赖转授的书写顺序不影响指纹。
        let mut a = Selection::default();
        let mut one = spec(Some("static"), None, None);
        one.deps
            .insert("libcurl".into(), vec!["ssl".into(), "nghttp2".into()]);
        a.per_component.insert("curl".into(), one);
        let mut b = Selection::default();
        let mut two = spec(Some("static"), None, None);
        two.deps
            .insert("libcurl".into(), vec!["nghttp2".into(), "ssl".into()]);
        b.per_component.insert("curl".into(), two);
        assert_eq!(
            scheme_digest(&a, &empty, &targets),
            scheme_digest(&b, &empty, &targets),
            "依赖 feature 顺序不应改变指纹"
        );
    }
}
