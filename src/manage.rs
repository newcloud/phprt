//! add / remove 的现场检查、交互和产物删除。
//!
//! 方案改动的决定权在 main：这里只回答“现场允不允许动”“要删哪些文件”，
//! 以及唯一需要问用户的地方（ini 被改过、静态组件要重编）。

use std::collections::BTreeSet;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::actions;
use crate::marker::{self, ComponentFacts, InstalledTree};
use crate::recipe::{BackupPolicy, Registry, Strategy, Target};

/// add 时的编译方式：显式写了就用；没写时内置组件必须显式选，其他用产品默认。
pub fn mode_for_add(
    reg: &Registry,
    product: &Target,
    series: &str,
    component: &Target,
    explicit: Option<&str>,
) -> Result<String> {
    if let Some(mode) = explicit {
        return Ok(mode.to_owned());
    }
    if component.source != "bundled" {
        return reg.default_mode(component);
    }
    let mut lines = vec![format!(
        "{} 是内置组件，add 时必须显式选择编译方式：",
        component.name
    )];
    for mode in &component.modes {
        let def = reg.product_mode(component, mode)?;
        let note = match def.strategy_kind()? {
            Strategy::Fold => "编进产品，需要重编",
            Strategy::Standalone => "编成动态扩展，不重编产品",
        };
        let name = &component.name;
        lines.push(format!(
            "  phprt add {}@{} '{name}=mode:{mode}'   # {note}",
            product.name, series
        ));
    }
    bail!("{}", lines.join("\n"))
}

/// 组件的 ini 相对路径；没有登记文件时返回 None。
pub fn ini_path(product: &Target, component: &Target, mode: &str) -> Option<String> {
    if component.ini.is_empty() {
        return None;
    }
    let register = product.profile.register.get(mode)?;
    let dir = product.profile.register_dir(register)?;
    Some(format!("{dir}/{}", component.ini))
}

/// add 之前的现场检查：目标 ini 是不是别人的文件、树里的产物还全不全。
pub fn check_add(
    product: &Target,
    component: &Target,
    mode: &str,
    tree: &Path,
    installed: Option<&InstalledTree>,
    force: bool,
) -> Result<()> {
    let Some(installed) = installed else {
        return Ok(());
    };

    if let Some(facts) = installed.component(&component.name) {
        let mut diffs: Vec<String> = missing(tree, facts)
            .into_iter()
            .map(|item| format!("缺少 {item}"))
            .collect();
        if diffs.is_empty() {
            for module in unloaded(product, tree, component)? {
                diffs.push(format!("模块 {module} 没有加载"));
            }
        }
        if !diffs.is_empty() && !force {
            bail!(
                "{} 在方案里（{}），磁盘不一致：\n  {}\n\
                 用 --force 重新编译并登记，或 phprt remove {}@{} {}",
                component.name,
                mode,
                diffs.join("\n  "),
                product.name,
                product.series,
                component.name
            );
        }
        return Ok(());
    }

    if !force
        && let Some(ini) = ini_path(product, component, mode)
        && tree.join(&ini).is_file()
    {
        bail!(
            "方案里没有 {}，但已装版本里已经有 {ini}\n\
             可能是你自己放的文件；确认要覆盖就加 --force",
            component.name
        );
    }
    Ok(())
}

/// remove 定下来的动作：删哪些文件、要不要重编产品。
pub struct Removal {
    pub facts: ComponentFacts,
    /// 要和产品一起重编的组件（fold）：不单独删产物，改完方案重编。
    pub fold: bool,
    /// 要删的相对路径，只含确实存在的文件。
    pub delete: Vec<String>,
}

/// remove 之前的检查：产物全不全、ini 有没有被改过、是不是静态组件。
pub fn remove_plan(
    reg: &Registry,
    component: &Target,
    installed: &InstalledTree,
    tree: &Path,
    force: bool,
) -> Result<Removal> {
    let facts = installed
        .component(&component.name)
        .cloned()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "已装版本里没有 {} 的记录；它可能没编过，先用 phprt install 补上",
                component.name
            )
        })?;
    let fold = reg.product_mode(component, &facts.mode)?.strategy_kind()? == Strategy::Fold;

    let gone = missing(tree, &facts);
    if !gone.is_empty() {
        if !force {
            bail!(
                "{} 的产物不全，不能直接删：\n  {}\n\
                 用 --force 只更新方案（不改磁盘）",
                component.name,
                gone.iter()
                    .map(|item| format!("缺少 {item}"))
                    .collect::<Vec<_>>()
                    .join("\n  ")
            );
        }
        return Ok(Removal {
            facts,
            fold,
            delete: Vec::new(),
        });
    }

    if !fold
        && !facts.ini.is_empty()
        && marker::file_digest(&tree.join(&facts.ini))? != facts.ini_hash
        && !force
    {
        let current = fs::read_to_string(tree.join(&facts.ini)).unwrap_or_default();
        let diff = ini_diff(&current, &actions::ini_text(component));
        let summary = if diff.is_empty() {
            "  （内容看起来一样，但指纹不同）".to_owned()
        } else {
            diff.join("\n")
        };
        let question = format!(
            "{} 被改过：\n{summary}\n删掉之后这些内容不会恢复。继续？",
            facts.ini
        );
        if !interactive() {
            bail!(
                "{} 被改过：\n{summary}\n删掉之后这些内容不会恢复；\
                 非交互环境用 --force 跳过检查",
                facts.ini
            );
        }
        if !confirm(&question)? {
            bail!("已取消；确认删除就再跑一次并回答 y，或用 --force");
        }
    }

    Ok(Removal {
        delete: delete_list(tree, &facts),
        facts,
        fold,
    })
}

/// 执行删除，返回删掉的相对路径。`backup` 为真时按产品规则先备份 ini。
pub fn apply(
    root: &Path,
    tree: &Path,
    removal: &Removal,
    policy: Option<&BackupPolicy>,
    backup: bool,
) -> Result<Vec<String>> {
    let mut deleted = Vec::new();
    for relative in &removal.delete {
        let path = tree.join(relative);
        if !path.is_file() {
            continue;
        }
        if backup
            && let Some(policy) = policy
            && !removal.facts.ini.is_empty()
            && relative == &removal.facts.ini
        {
            backup_file(
                root,
                policy,
                &removal.facts.name,
                &removal.facts.mode,
                &path,
                None,
            )?;
        }
        fs::remove_file(&path).with_context(|| format!("删不了 {}", path.display()))?;
        deleted.push(relative.clone());
    }
    Ok(deleted)
}

/// 按产品 profile 的规则备份一个组件登记文件。
///
/// 文件不存在、或内容和将要写入的一致时都不备份；备份失败直接报错，
/// 提示可以用 `--no-backup-php-ext-ini` 跳过。
pub fn backup_file(
    root: &Path,
    policy: &BackupPolicy,
    component: &str,
    mode: &str,
    source: &Path,
    expected: Option<&str>,
) -> Result<Option<PathBuf>> {
    if !source.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(source).with_context(|| format!("读不了 {}", source.display()))?;
    if let Some(expected) = expected
        && expected.as_bytes() == bytes.as_slice()
    {
        return Ok(None);
    }

    let saved = (|| -> Result<PathBuf> {
        let dir = match crate::layout::backup_dir_name() {
            Some(value) => crate::layout::resolve(root, value),
            None => crate::layout::internal_dir(root).join(&policy.relative_dir),
        };
        fs::create_dir_all(&dir).with_context(|| format!("建不了 {}", dir.display()))?;
        let stamp = stamp()?;
        let name = expand_backup_name(&policy.name, component, mode, &stamp);
        let path = unique_path(&dir, &name);
        fs::write(&path, &bytes).with_context(|| format!("写不了 {}", path.display()))?;
        Ok(path)
    })();

    match saved {
        Ok(path) => {
            println!("已备份 ini: {}", path.display());
            Ok(Some(path))
        }
        Err(err) => Err(err.context("备份 ini 失败；可以加 --no-backup-php-ext-ini 跳过备份")),
    }
}

fn expand_backup_name(template: &str, component: &str, mode: &str, stamp: &str) -> String {
    template
        .replace("{timestamp}", stamp)
        .replace("{component}", component)
        .replace("{mode}", mode)
}

/// 同名文件依次加 `-2`、`-3`；序号放在扩展名前面。
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(name);
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_owned(), format!(".{ext}")),
        None => (name.to_owned(), String::new()),
    };
    let mut index = 1;
    while path.exists() {
        index += 1;
        path = dir.join(format!("{stem}-{index}{ext}"));
    }
    path
}

/// 备份文件名里的日期时间，精确到秒，和文档里的 `date +%Y%m%d-%H%M%S` 一致。
fn stamp() -> Result<String> {
    let output = Command::new("date")
        .arg("+%Y%m%d-%H%M%S")
        .output()
        .context("跑不起来 date")?;
    if !output.status.success() {
        bail!("date 退出 {}，拿不到当前时间", output.status);
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if text.is_empty() {
        bail!("date 没有输出，拿不到当前时间");
    }
    Ok(text)
}

/// 会重编产品时给用户的提示：说明代价，并给出自行备份的命令（不代做备份）。
/// fold 只表示“和产品一起构建”，不等于静态链接（nginx 动态模块也是 fold）。
pub fn rebuild_warning(product: &Target, name: &str, mode: &str, tree: &str) -> String {
    format!(
        "{name}（{mode}）需要和 {} 一起重新 configure/make（约 3 分钟），\
         并覆盖 {tree}/ 里的文件\n建议先自行备份：\n  cp -a {tree} {tree}.bak-$(date +%F-%H%M)",
        product.name
    )
}

/// 重编产品之后，删掉已经不属于方案的旧产物（如 nginx 动态模块的 `.so`）。
/// fold 组件本身没有源码级产物时（PHP static），这里什么也不做。
pub fn delete_stale_artifacts(tree: &Path, artifacts: &[String]) -> Result<Vec<String>> {
    let mut deleted = Vec::new();
    for relative in artifacts {
        // nginx 的 make install 会把旧模块改名成 `<名字>.old`，一起清掉。
        for candidate in [relative.clone(), format!("{relative}.old")] {
            let path = tree.join(&candidate);
            if path.is_file() {
                fs::remove_file(&path).with_context(|| format!("删不了 {}", path.display()))?;
                deleted.push(candidate);
            }
        }
    }
    Ok(deleted)
}

/// 有没有可以问人的终端。没有就一律拒绝，不等输入。
pub fn interactive() -> bool {
    std::io::stdin().is_terminal()
}

/// TTY 下问一句 `[y/N]`；非 TTY 直接返回 false，绝不阻塞等待输入。
pub fn confirm(question: &str) -> Result<bool> {
    if !interactive() {
        return Ok(false);
    }
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let line = line.trim();
    Ok(matches!(line, "y" | "Y" | "yes" | "YES"))
}

/// 树里记的产物和 ini 哪些不见了。
pub fn missing(tree: &Path, facts: &ComponentFacts) -> Vec<String> {
    let mut out = Vec::new();
    for artifact in &facts.artifacts {
        if !tree.join(artifact).exists() {
            out.push(artifact.clone());
        }
    }
    if !facts.ini.is_empty() && !tree.join(&facts.ini).is_file() {
        out.push(facts.ini.clone());
    }
    out
}

fn delete_list(tree: &Path, facts: &ComponentFacts) -> Vec<String> {
    let mut out: Vec<String> = facts
        .artifacts
        .iter()
        .filter(|relative| tree.join(relative).is_file())
        .cloned()
        .collect();
    if !facts.ini.is_empty() && tree.join(&facts.ini).is_file() {
        out.push(facts.ini.clone());
    }
    out
}

/// 产品启动后没看到的模块。
fn unloaded(product: &Target, tree: &Path, component: &Target) -> Result<Vec<String>> {
    if component.modules.is_empty() || product.profile.verify_modules.is_empty() {
        return Ok(Vec::new());
    }
    let command = product
        .profile
        .verify_modules
        .replace("{tree}", &tree.display().to_string());
    let output = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .with_context(|| format!("跑不起来 {command}"))?;
    if !output.status.success() {
        bail!(
            "{command} 失败：\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let loaded: BTreeSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_ascii_lowercase())
        .filter(|line| !line.is_empty() && !line.starts_with('['))
        .collect();
    Ok(component
        .modules
        .iter()
        .filter(|module| !loaded.contains(&module.to_ascii_lowercase()))
        .cloned()
        .collect())
}

/// 当前 ini 和 phprt 生成的内容相比，少了哪些行、多了哪些行。
fn ini_diff(current: &str, expected: &str) -> Vec<String> {
    let current: BTreeSet<&str> = current
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let expected: BTreeSet<&str> = expected
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut out = Vec::new();
    for line in &expected {
        if !current.contains(line) {
            out.push(format!("  - {line}"));
        }
    }
    for line in &current {
        if !expected.contains(line) {
            out.push(format!("  + {line}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{Action, Kind, Profile, ProfileMode};

    fn blank() -> Target {
        Target {
            kind: Kind::Component,
            name: String::new(),
            version: String::new(),
            url: String::new(),
            sha256: String::new(),
            needs: vec![],
            tools: vec![],
            host_tools: vec![],
            bundled: vec![],
            bundled_disable: Default::default(),
            components: vec![],
            deps: Default::default(),
            profile: Default::default(),
            product: String::new(),
            source: String::new(),
            modes: vec![],
            default_mode: String::new(),
            source_path: String::new(),
            modules: vec![],
            ini: String::new(),
            ini_lines: vec![],
            env: Default::default(),
            dep_configure: Default::default(),
            prepare: vec![],
            features: Default::default(),
            ignored: Default::default(),
            configure: vec![],
            make: vec![],
            artifacts: vec![],
            desc: String::new(),
            line: String::new(),
            series: String::new(),
        }
    }

    fn component(name: &str, source: &str, modes: &[&str]) -> Target {
        Target {
            name: name.into(),
            product: "php".into(),
            source: source.into(),
            modes: modes.iter().map(|item| item.to_string()).collect(),
            default_mode: "shared".into(),
            modules: vec![name.into()],
            ini: format!("27-{name}.ini"),
            ..blank()
        }
    }

    fn product() -> Target {
        let mut profile = Profile::default();
        for (mode, strategy) in [("static", "fold"), ("shared", "standalone")] {
            profile.modes.insert(
                mode.to_string(),
                ProfileMode {
                    strategy: strategy.to_string(),
                    ..ProfileMode::default()
                },
            );
        }
        profile.defaults.insert("bundled".into(), "static".into());
        profile.defaults.insert("external".into(), "shared".into());
        profile.register.insert("shared".into(), "php-ini".into());
        profile.post_install.push(Action::WriteComponentIni {
            dir: "etc/conf.d".into(),
            register: "php-ini".into(),
        });
        Target {
            kind: Kind::Product,
            name: "php".into(),
            product: "php".into(),
            series: "8.5".into(),
            profile,
            ..blank()
        }
    }

    fn registry(product: &Target, component: &Target) -> Registry {
        let mut targets = std::collections::BTreeMap::new();
        targets.insert(product.id(), product.clone());
        targets.insert(component.id(), component.clone());
        Registry { targets }
    }

    #[test]
    fn bundled_without_mode_lists_both_modes() {
        let product = product();
        let gd = component("gd", "bundled", &["static", "shared"]);
        let reg = registry(&product, &gd);
        let err = mode_for_add(&reg, &product, "8.5", &gd, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("必须显式选择编译方式"), "{err}");
        assert!(err.contains("phprt add php@8.5 'gd=mode:static'"), "{err}");
        assert!(err.contains("phprt add php@8.5 'gd=mode:shared'"), "{err}");
        assert!(err.contains("需要重编"), "{err}");
    }

    #[test]
    fn external_without_mode_uses_profile_default() {
        let product = product();
        let swoole = component("swoole", "external", &["shared"]);
        let reg = registry(&product, &swoole);
        assert_eq!(
            mode_for_add(&reg, &product, "8.5", &swoole, None).unwrap(),
            "shared"
        );
        assert_eq!(
            mode_for_add(&reg, &product, "8.5", &swoole, Some("shared")).unwrap(),
            "shared"
        );
    }

    #[test]
    fn ini_path_follows_register_dir() {
        let product = product();
        let swoole = component("swoole", "external", &["shared"]);
        assert_eq!(
            ini_path(&product, &swoole, "shared").as_deref(),
            Some("etc/conf.d/27-swoole.ini")
        );
        assert_eq!(ini_path(&product, &swoole, "static"), None);
    }

    #[test]
    fn ini_diff_shows_added_and_missing_lines() {
        let expected = actions::ini_text(&component("swoole", "external", &["shared"]));
        let current = format!("{expected}ini_extra=1\n");
        assert_eq!(
            ini_diff(&current, &expected),
            vec!["  + ini_extra=1".to_owned()]
        );

        let diff = ini_diff("extension=x.so\n", &expected);
        assert!(
            diff.contains(&"  - extension=swoole.so".to_owned()),
            "{diff:?}"
        );
        assert!(diff.contains(&"  + extension=x.so".to_owned()), "{diff:?}");
    }

    #[test]
    fn add_refuses_foreign_ini_unless_forced() {
        let root = std::env::temp_dir().join(format!("phprt-add-ini-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("etc/conf.d")).unwrap();
        fs::write(root.join("etc/conf.d/27-swoole.ini"), "手放的\n").unwrap();

        let product = product();
        let swoole = component("swoole", "external", &["shared"]);
        let installed = InstalledTree {
            version: "8.5.12".into(),
            scheme: "s".into(),
            resolution: String::new(),
            scheme_cmd: String::new(),
            components: Vec::new(),
        };
        let err = check_add(&product, &swoole, "shared", &root, Some(&installed), false)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("已装版本里已经有 etc/conf.d/27-swoole.ini"),
            "{err}"
        );
        assert!(err.contains("--force"), "{err}");
        // 目标 ini 不存在时正常放行；--force 也放行。
        check_add(&product, &swoole, "shared", &root, Some(&installed), true).unwrap();
        fs::remove_file(root.join("etc/conf.d/27-swoole.ini")).unwrap();
        check_add(&product, &swoole, "shared", &root, Some(&installed), false).unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn add_reports_missing_artifacts_when_already_installed() {
        let root = std::env::temp_dir().join(format!("phprt-add-missing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("etc/conf.d")).unwrap();
        fs::write(root.join("etc/conf.d/27-swoole.ini"), "x").unwrap();
        let product = product();
        let swoole = component("swoole", "external", &["shared"]);
        let installed = InstalledTree {
            version: "8.5.12".into(),
            scheme: "s".into(),
            resolution: String::new(),
            scheme_cmd: String::new(),
            components: vec![ComponentFacts {
                name: "swoole".into(),
                mode: "shared".into(),
                ini: "etc/conf.d/27-swoole.ini".into(),
                ini_hash: "h".into(),
                artifacts: vec!["lib/php/extensions/x/swoole.so".into()],
            }],
        };
        let err = check_add(&product, &swoole, "shared", &root, Some(&installed), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("磁盘不一致"), "{err}");
        assert!(err.contains("lib/php/extensions/x/swoole.so"), "{err}");
        check_add(&product, &swoole, "shared", &root, Some(&installed), true).unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn deletes_stale_artifacts_after_rebuild() {
        let root = std::env::temp_dir().join(format!("phprt-stale-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("modules")).unwrap();
        fs::write(root.join("modules/ngx_http_lua_module.so"), b"so").unwrap();
        fs::write(root.join("modules/ngx_http_lua_module.so.old"), b"so").unwrap();
        fs::write(root.join("modules/keep.so"), b"so").unwrap();

        let deleted = delete_stale_artifacts(
            &root,
            &[
                "modules/ngx_http_lua_module.so".to_owned(),
                "modules/gone.so".to_owned(),
            ],
        )
        .unwrap();
        assert_eq!(
            deleted,
            vec![
                "modules/ngx_http_lua_module.so",
                "modules/ngx_http_lua_module.so.old"
            ]
        );
        assert!(!root.join("modules/ngx_http_lua_module.so").exists());
        assert!(!root.join("modules/ngx_http_lua_module.so.old").exists());
        assert!(root.join("modules/keep.so").exists());
        let _ = fs::remove_dir_all(&root);
    }

    fn policy() -> BackupPolicy {
        BackupPolicy {
            relative_dir: "backup".into(),
            name: "{timestamp}_{component}.ini".into(),
        }
    }

    #[test]
    fn backup_name_follows_timestamp_and_component() {
        assert_eq!(
            expand_backup_name(
                "{timestamp}_{component}.ini",
                "swoole",
                "shared",
                "20261117-003012"
            ),
            "20261117-003012_swoole.ini"
        );
        assert_eq!(
            expand_backup_name("{component}-{mode}.ini", "gd", "shared", "x"),
            "gd-shared.ini"
        );

        // 同名依次加 -2、-3，序号在扩展名前面。
        let dir = std::env::temp_dir().join(format!("phprt-backup-name-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "a.ini"), dir.join("a.ini"));
        fs::write(dir.join("a.ini"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a.ini"), dir.join("a-2.ini"));
        fs::write(dir.join("a-2.ini"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a.ini"), dir.join("a-3.ini"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_skips_missing_or_unchanged_ini() {
        let root = std::env::temp_dir().join(format!("phprt-backup-skip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let policy = policy();

        // 文件不存在：不备份。
        assert!(
            backup_file(
                &root,
                &policy,
                "swoole",
                "shared",
                &root.join("none.ini"),
                None
            )
            .unwrap()
            .is_none()
        );

        // 内容与将要写入的一致：不备份。
        let ini = root.join("27-swoole.ini");
        fs::write(&ini, "extension=swoole.so\n").unwrap();
        assert!(
            backup_file(
                &root,
                &policy,
                "swoole",
                "shared",
                &ini,
                Some("extension=swoole.so\n")
            )
            .unwrap()
            .is_none()
        );

        // 内容不同：备份到 <root>/backup/<时间>_<组件>.ini。
        let saved = backup_file(
            &root,
            &policy,
            "swoole",
            "shared",
            &ini,
            Some("extension=x.so\n"),
        )
        .unwrap()
        .unwrap();
        assert!(saved.starts_with(root.join(".phprt/backup")), "{saved:?}");
        let name = saved.file_name().unwrap().to_string_lossy();
        assert!(name.ends_with("_swoole.ini"), "{name}");
        assert_eq!(fs::read(&saved).unwrap(), b"extension=swoole.so\n".to_vec());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn backup_failure_mentions_skip_flag() {
        let root = std::env::temp_dir().join(format!("phprt-backup-fail-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        // 让备份目录变成一个文件，建目录必然失败。
        fs::create_dir_all(root.join(".phprt")).unwrap();
        fs::write(root.join(".phprt/backup"), "not a dir").unwrap();
        let ini = root.join("27-swoole.ini");
        fs::write(&ini, "x").unwrap();
        let err = backup_file(&root, &policy(), "swoole", "shared", &ini, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--no-backup-php-ext-ini"), "{err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_backs_up_ini_before_delete() {
        let root = std::env::temp_dir().join(format!("phprt-apply-{}", std::process::id()));
        let tree = root.join(".phprt/php-8.5.12");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(tree.join("etc/conf.d")).unwrap();
        fs::create_dir_all(tree.join("lib/php/extensions/x")).unwrap();
        fs::write(tree.join("etc/conf.d/27-swoole.ini"), "手改过\n").unwrap();
        fs::write(tree.join("lib/php/extensions/x/swoole.so"), "elf").unwrap();

        let removal = Removal {
            facts: ComponentFacts {
                name: "swoole".into(),
                mode: "shared".into(),
                ini: "etc/conf.d/27-swoole.ini".into(),
                ini_hash: "h".into(),
                artifacts: vec!["lib/php/extensions/x/swoole.so".into()],
            },
            fold: false,
            delete: vec![
                "lib/php/extensions/x/swoole.so".into(),
                "etc/conf.d/27-swoole.ini".into(),
            ],
        };
        let policy = policy();
        let deleted = apply(&root, &tree, &removal, Some(&policy), true).unwrap();
        assert_eq!(deleted.len(), 2);
        assert!(!tree.join("etc/conf.d/27-swoole.ini").exists());
        let saved: Vec<_> = fs::read_dir(root.join(".phprt/backup"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        assert_eq!(saved.len(), 1);
        assert_eq!(fs::read(&saved[0]).unwrap(), "手改过\n".as_bytes().to_vec());

        // 关掉备份时不写文件。
        fs::write(tree.join("etc/conf.d/27-swoole.ini"), "x").unwrap();
        let deleted = apply(&root, &tree, &removal, Some(&policy), false).unwrap();
        assert_eq!(deleted, vec!["etc/conf.d/27-swoole.ini".to_owned()]);
        let saved: Vec<_> = fs::read_dir(root.join(".phprt/backup"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(saved.len(), 1, "跳过备份时不应新增文件");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_lists_gone_artifacts_and_ini() {
        let root = std::env::temp_dir().join(format!("phprt-missing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("etc/conf.d")).unwrap();
        fs::write(root.join("etc/conf.d/27-swoole.ini"), "x").unwrap();
        let facts = ComponentFacts {
            name: "swoole".into(),
            mode: "shared".into(),
            ini: "etc/conf.d/27-swoole.ini".into(),
            ini_hash: "h".into(),
            artifacts: vec!["lib/php/extensions/x/swoole.so".into()],
        };
        assert_eq!(
            missing(&root, &facts),
            vec!["lib/php/extensions/x/swoole.so".to_owned()]
        );
        let _ = fs::remove_dir_all(&root);
    }
}
