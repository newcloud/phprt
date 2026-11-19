use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::manage;
use crate::recipe::{Action, Kind, Registry, Selection, Target, check_relative};

/// 安装后动作的运行参数。
pub struct Options<'a> {
    /// 项目根；备份目录相对它。
    pub root: &'a Path,
    /// 是否按产品 profile 的规则备份被改写的登记文件。
    pub backup: bool,
}

/// 按 product.profile.post_install 执行安装后动作。
pub fn run(
    reg: &Registry,
    product: &Target,
    sel: &Selection,
    tree: &Path,
    opts: &Options<'_>,
) -> Result<()> {
    for action in &product.profile.post_install {
        run_action(action, reg, product, sel, tree, opts)?;
    }
    for (name, target) in &product.targets {
        if !product.selected_targets.contains_key(name) {
            continue;
        }
        for action in &target.post_install {
            run_action(action, reg, product, sel, tree, opts)?;
        }
    }
    Ok(())
}

fn run_action(
    action: &Action,
    reg: &Registry,
    product: &Target,
    sel: &Selection,
    tree: &Path,
    opts: &Options<'_>,
) -> Result<()> {
    match action {
        Action::EnsureDir { path } => {
            let path = safe_path(tree, path)?;
            fs::create_dir_all(&path).with_context(|| format!("建不了 {}", path.display()))?;
        }
        Action::WriteFile {
            path,
            content,
            if_missing,
        } => {
            let path = safe_path(tree, path)?;
            if *if_missing && path.exists() {
                return Ok(());
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("建不了 {}", parent.display()))?;
            }
            fs::write(&path, content).with_context(|| format!("写不了 {}", path.display()))?;
        }
        Action::CopyCaBundle { to, candidates } => {
            let to = safe_path(tree, to)?;
            if to.exists() {
                return Ok(());
            }
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("建不了 {}", parent.display()))?;
            }
            if let Some(source) = candidates.iter().find(|path| Path::new(path).is_file()) {
                fs::copy(source, &to).with_context(|| format!("复制不了 CA {}", source))?;
            } else {
                eprintln!("注意: 没找到系统 CA，HTTPS 校验会失败");
            }
        }
        Action::WriteComponentIni { dir, register } => {
            write_component_ini(reg, product, sel, tree, dir, register, opts)?;
        }
    }
    Ok(())
}

/// 组件 ini 的内容。写入和“有没有被手改”的比对共用一份。
pub fn ini_text(component: &Target) -> String {
    let lines = if component.ini_lines.is_empty() {
        vec![format!("extension={}.so", component.name)]
    } else {
        component.ini_lines.clone()
    };
    let mut text = String::from("; 由 phprt 生成，勿手改\n");
    for line in lines {
        text.push_str(&line);
        text.push('\n');
    }
    text
}

fn write_component_ini(
    reg: &Registry,
    product: &Target,
    sel: &Selection,
    tree: &Path,
    dir: &str,
    register: &str,
    opts: &Options<'_>,
) -> Result<()> {
    let dir = safe_path(tree, dir)?;
    fs::create_dir_all(&dir).with_context(|| format!("建不了 {}", dir.display()))?;

    for component in reg.of_kind(Kind::Component) {
        if component.ini.is_empty() {
            continue;
        }
        let mode = sel.mode_of(component);
        let enabled = product.components.contains(&component.name)
            && product.profile.register.get(&mode).map(String::as_str) == Some(register);
        let path = dir.join(&component.ini);
        if enabled {
            let text = ini_text(component);
            back_up(opts, product, component, &mode, &path, Some(&text))?;
            fs::write(&path, text).with_context(|| format!("写不了 {}", path.display()))?;
        } else if path.exists() {
            back_up(opts, product, component, &mode, &path, None)?;
            fs::remove_file(&path).with_context(|| format!("删不了旧 ini {}", path.display()))?;
        }
    }
    Ok(())
}

/// 产品声明了备份规则、且本次没有被关掉时，先存一份再改写或删除。
fn back_up(
    opts: &Options<'_>,
    product: &Target,
    component: &Target,
    mode: &str,
    path: &Path,
    expected: Option<&str>,
) -> Result<()> {
    if let Some(policy) = &product.profile.backup
        && opts.backup
    {
        manage::backup_file(opts.root, policy, &component.name, mode, path, expected)?;
    }
    Ok(())
}

fn safe_path(tree: &Path, relative: &str) -> Result<PathBuf> {
    check_relative(relative, "安装后动作的路径")?;
    Ok(tree.join(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_must_stay_inside_the_tree() {
        let tree = Path::new("/tmp/tree");
        assert_eq!(
            safe_path(tree, "etc/conf.d").unwrap(),
            PathBuf::from("/tmp/tree/etc/conf.d")
        );
        assert!(safe_path(tree, "/etc/passwd").is_err());
        assert!(safe_path(tree, "../x").is_err());
        assert!(safe_path(tree, "").is_err());
    }
}
