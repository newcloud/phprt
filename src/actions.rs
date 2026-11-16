use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::recipe::{Action, Kind, Registry, Selection, Target, check_relative};

/// 按 product.profile.post_install 执行安装后动作。
pub fn run(reg: &Registry, product: &Target, sel: &Selection, tree: &Path) -> Result<()> {
    for action in &product.profile.post_install {
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
                    continue;
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
                    continue;
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
                write_component_ini(reg, product, sel, tree, dir, register)?;
            }
        }
    }
    Ok(())
}

fn write_component_ini(
    reg: &Registry,
    product: &Target,
    sel: &Selection,
    tree: &Path,
    dir: &str,
    register: &str,
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
            fs::write(&path, text).with_context(|| format!("写不了 {}", path.display()))?;
        } else if path.exists() {
            fs::remove_file(&path).with_context(|| format!("删不了旧 ini {}", path.display()))?;
        }
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
