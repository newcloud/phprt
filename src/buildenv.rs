//! 构建环境调度：用户级镜像缓存、实例创建、命令转发、容器内装包。
//!
//! 这里只做 phprt 入口层的判断和转发；镜像/容器本身由 `phprt-buildimages`
//! 负责。容器内的 phprt 通过 `PHPRT_IN_CONTAINER=1` 识别自己，不再递归转发。

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use phprt_buildimages::{
    InstanceRequest, MountSpec, Store, instance_id, normalize_digest, short_digest,
};

use crate::bundle::Bundle;
use crate::catalog::Catalog;
use crate::host_commands;
use crate::layout;
use crate::state::{self, BuildState, ComponentState, ProjectState};
use crate::{BuildimagesCmd, Cli, Cmd};

pub const IN_CONTAINER: &str = "PHPRT_IN_CONTAINER";
pub const RUNTIME: &str = "youki";

pub fn in_container() -> bool {
    env::var(IN_CONTAINER).as_deref() == Ok("1")
}

pub fn signature_key(project: &ProjectState) -> String {
    project
        .build
        .as_ref()
        .map(BuildState::signature_key)
        .unwrap_or_else(|| "host".to_string())
}

pub fn run_container_exec(
    instance: &str,
    workdir: &Path,
    env_pairs: &[String],
    command: &[String],
) -> Result<()> {
    let mut env_map = std::collections::HashMap::new();
    for item in env_pairs {
        let (key, value) = item
            .split_once('=')
            .ok_or_else(|| anyhow!("--env 需要 KEY=VALUE，实际是 {item}"))?;
        env_map.insert(key.to_string(), value.to_string());
    }
    let code = phprt_buildimages::exec(instance, workdir, env_map, command.to_vec())?;
    std::process::exit(code);
}

pub fn handle_buildimages(cli: &Cli, command: &BuildimagesCmd, recipes: &Path) -> Result<()> {
    if in_container() {
        bail!("buildimages 只能在宿主上运行");
    }
    if layout::is_global()
        && matches!(
            command,
            BuildimagesCmd::Use { .. } | BuildimagesCmd::Rm { .. }
        )
    {
        bail!("buildimages use / rm 只支持本地模式");
    }
    let store = Store::open()?;
    match command {
        BuildimagesCmd::Add {
            reference,
            name,
            offline,
        } => {
            let image = store.add_image(reference, name.as_deref(), *offline)?;
            println!();
            println!("镜像      {}", image.reference);
            println!("digest    {}", image.digest);
            if !image.aliases.is_empty() {
                println!("别名      {}", image.aliases.join(", "));
            }
            println!("平台      {}", image.platform);
            println!("大小      {:.1} MB", image.size as f64 / 1024.0 / 1024.0);
            println!();
            println!("下一步：");
            println!(
                "  phprt buildimages use <产品>@<系列> --image {}",
                short_digest(&image.digest)
            );
            println!(
                "  或 phprt install <产品>@<系列> --image {}",
                short_digest(&image.digest)
            );
        }
        BuildimagesCmd::List => {
            let images = store.list()?;
            if images.is_empty() {
                println!("本地还没有构建镜像。先执行 phprt buildimages add <镜像引用>");
                return Ok(());
            }
            println!(
                "{:<14} {:<46} {:<12} {:<12} {:>8}  refs",
                "digest", "alias", "platform", "created", "size"
            );
            for image in images {
                println!(
                    "{:<14} {:<46} {:<12} {:<12} {:>7.1}M  {}",
                    short_digest(&image.digest),
                    image.aliases.join(","),
                    image.platform,
                    image.created,
                    image.size as f64 / 1024.0 / 1024.0,
                    image.refs.join(", ")
                );
            }
        }
        BuildimagesCmd::Use {
            target,
            image,
            host,
            force,
        } => {
            if *host == image.is_some() {
                bail!("use 需要 --image <镜像> 或 --host 二选一");
            }
            let (product, series) = resolve_target(target, recipes)?;
            let selection = if *host {
                None
            } else {
                let query = image.as_deref().expect("clap 已保证");
                let info = store.resolve(query)?;
                Some(BuildState {
                    mode: "oci".to_string(),
                    image: info.digest,
                    image_ref: info.reference,
                    runtime: RUNTIME.to_string(),
                    instance: String::new(),
                    instance_created: String::new(),
                })
            };
            let state_file = state::state_path(&cli.root, &product, &series);
            let mut project = ProjectState::load(&state_file)?;
            project.check(&product, &series)?;
            let mut selection = selection;
            if let Some(new) = selection.as_mut()
                && let Some(old) = project.build.as_ref().filter(|old| old.image == new.image)
            {
                new.instance = old.instance.clone();
                new.instance_created = old.instance_created.clone();
            }
            let current = project.build.as_ref().map(BuildState::signature_key);
            let wanted = selection.as_ref().map(BuildState::signature_key);
            if !project.installed_version.is_empty() && current != wanted && !force {
                bail!(
                    "{}@{} 已经安装过；切换构建环境会让产物重编。\n  \
                     确认要换请加 --force，或先用 phprt install {}@{} --force",
                    product,
                    series,
                    product,
                    series
                );
            }
            if !project.active {
                seed_default_components(&mut project, recipes, &product, &series)?;
            }
            project.build = selection;
            project.save(&state_file)?;
            println!(
                "{}@{} 的构建环境：{}",
                product,
                series,
                project
                    .build
                    .as_ref()
                    .map(|build| format!("{} {}", short_digest(&build.image), build.image_ref))
                    .unwrap_or_else(|| "宿主".to_string())
            );
            println!("已写入 {}", state_file.display());
            println!("下一次 phprt install {}@{} 生效", product, series);
        }
        BuildimagesCmd::Rm { image, force } => {
            let referenced = referenced_digests(&cli.root);
            let instances = store.remove(image, &referenced, *force)?;
            println!("已删除镜像 {image}");
            if !instances.is_empty() {
                println!("以下实例仍保留旧 rootfs，但不能再从镜像重建：");
                for id in instances {
                    println!("  {id}");
                }
            }
        }
        BuildimagesCmd::Gc { dry_run } => {
            let removed = store.gc(*dry_run)?;
            if removed.is_empty() {
                println!("没有需要清理的缓存");
            } else {
                for path in removed {
                    if *dry_run {
                        println!("将删除 {}", path.display());
                    } else {
                        println!("已删除 {}", path.display());
                    }
                }
            }
        }
    }
    Ok(())
}

/// 返回 true 表示命令已经转发进容器并处理完退出码。
pub fn maybe_forward(cli: &Cli, recipes: &Path) -> Result<bool> {
    if in_container() {
        return Ok(false);
    }
    let (target, cli_image, offline, plan, force) = match &cli.cmd {
        Cmd::Plan { target, image, .. } => (target.clone(), image.clone(), false, true, false),
        Cmd::Install {
            target,
            image,
            download,
            force,
            ..
        } => {
            let target = target.first().cloned().unwrap_or_default();
            (target, image.clone(), download.offline, false, *force)
        }
        Cmd::Update {
            target,
            image,
            download,
            ..
        } => (
            target.first().cloned().unwrap_or_default(),
            image.clone(),
            download.offline,
            false,
            false,
        ),
        Cmd::Add {
            target,
            image,
            download,
            ..
        } => (
            target.clone(),
            image.clone(),
            download.offline,
            false,
            false,
        ),
        Cmd::Remove {
            target, download, ..
        } => (target.clone(), None, download.offline, false, false),
        _ => return Ok(false),
    };
    if target.is_empty() {
        return Ok(false);
    }
    let (product, series) = resolve_target(&target, recipes)?;
    let state_file = state::state_path(&cli.root, &product, &series);
    let mut project = ProjectState::load(&state_file)?;
    project.check(&product, &series)?;

    let recorded = project.build.clone().filter(|build| build.is_oci());
    if recorded.is_none() && cli_image.is_none() {
        return Ok(false);
    }
    if layout::is_global() {
        bail!("构建镜像只支持本地模式，不能和 --global --prefix 一起用");
    }

    let store = Store::open()?;
    let image = if let Some(query) = &cli_image {
        let info = resolve_or_pull(&store, query, offline)?;
        if let Some(current) = &recorded
            && current.image != info.digest
            && !project.installed_version.is_empty()
            && !force
        {
            bail!(
                "{}@{} 已经用镜像 {} 安装过；切换镜像请加 --force，\n  \
                 或先 phprt buildimages use {}@{} --image {} --force",
                product,
                series,
                short_digest(&current.image),
                product,
                series,
                short_digest(&info.digest)
            );
        }
        info
    } else {
        let current = recorded.as_ref().expect("recorded 一定存在");
        let query = if current.image_ref.is_empty() {
            current.image.clone()
        } else {
            current.image_ref.clone()
        };
        let info = resolve_or_pull(&store, &query, offline)?;
        if info.digest != current.image {
            bail!(
                "{}@{} 记录的镜像 {} 和本地/远端解析到的 {} 不一致；\n  \
                 tag 可能已经前进，确认切换请 phprt buildimages use {}@{} --image {} --force",
                product,
                series,
                short_digest(&current.image),
                short_digest(&info.digest),
                product,
                series,
                short_digest(&info.digest)
            );
        }
        info
    };

    let mut build = BuildState {
        mode: "oci".to_string(),
        image: image.digest.clone(),
        image_ref: image.reference.clone(),
        runtime: RUNTIME.to_string(),
        instance: String::new(),
        instance_created: String::new(),
    };

    if !plan {
        let current = project.build.as_ref().map(BuildState::signature_key);
        let wanted = Some(build.signature_key());
        if !project.installed_version.is_empty() && current != wanted && !force {
            bail!(
                "{}@{} 已经安装过；切换构建环境会让产物重编。\n  \
                 确认要换请加 --force",
                product,
                series
            );
        }
        if !project.active {
            seed_default_components(&mut project, recipes, &product, &series)?;
        }
        project.build = Some(build.clone());
        project.save(&state_file)?;
    }

    let root = std::fs::canonicalize(&cli.root)
        .with_context(|| format!("读不到项目根 {}", cli.root.display()))?;
    let id = instance_id(&root, &product, &series, &image.digest);
    let request = InstanceRequest {
        id: id.clone(),
        image: image.digest.clone(),
        project_root: root.clone(),
        mounts: mount_specs(cli, recipes, &root),
        executable: env::current_exe().context("读不到当前 phprt 可执行文件")?,
    };
    let instance = phprt_buildimages::ensure_instance(&store, &request)?;
    probe_instance(&instance.id, &root)?;

    build.instance = instance.id.clone();
    if !plan {
        project.build = Some(build);
        project.save(&state_file)?;
    }

    let paths = ForwardPaths {
        root: root.clone(),
        recipes: std::fs::canonicalize(recipes).unwrap_or_else(|_| recipes.to_path_buf()),
        downloads: layout::downloads_dir(&root),
        internal: layout::internal_dir(&root),
        build: resolve_dir(&root, layout::build_dir_name()),
        backup: layout::backup_dir_name().map(|name| resolve_dir(&root, name)),
        config: std::fs::canonicalize(effective_config_path(cli)).ok(),
    };
    let status = forward(&instance.id, &root, &paths)?;
    std::process::exit(status);
}

struct ForwardPaths {
    root: PathBuf,
    recipes: PathBuf,
    downloads: PathBuf,
    internal: PathBuf,
    build: PathBuf,
    backup: Option<PathBuf>,
    config: Option<PathBuf>,
}

fn effective_config_path(cli: &Cli) -> PathBuf {
    match &cli.config {
        Some(path) if !path.as_os_str().is_empty() => path.clone(),
        _ => cli.root.join("phprt.toml"),
    }
}

fn forward(instance: &str, root: &Path, paths: &ForwardPaths) -> Result<i32> {
    let exe = env::current_exe().context("读不到当前 phprt 可执行文件")?;
    let mut command = Command::new(exe);
    command
        .arg("__container-exec")
        .arg("--instance")
        .arg(instance)
        .arg("--workdir")
        .arg(root)
        .arg("--env")
        .arg(format!("{IN_CONTAINER}=1"))
        .arg("--env")
        .arg(format!(
            "PATH={}",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
        ))
        .arg("--env")
        .arg("HOME=/root");
    command.arg("--").arg("/phprt");
    for arg in forwarded_args() {
        command.arg(arg);
    }
    command.arg("--root").arg(&paths.root);
    command.arg("--recipes").arg(&paths.recipes);
    command.arg("--download-dir").arg(&paths.downloads);
    command.arg("--internal-dir").arg(&paths.internal);
    command.arg("--build-dir").arg(&paths.build);
    if let Some(backup) = &paths.backup {
        command.arg("--backup-dir").arg(backup);
    }
    if let Some(config) = &paths.config {
        command.arg("--config").arg(config);
    }
    let status = command.status().context("转发命令到构建实例失败")?;
    Ok(status.code().unwrap_or(1))
}

fn forwarded_args() -> Vec<String> {
    filter_forward_args(env::args().skip(1))
}

fn filter_forward_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    const PATH_FLAGS: &[&str] = &[
        "--root",
        "--recipes",
        "--download-dir",
        "--internal-dir",
        "--build-dir",
        "--backup-dir",
        "-c",
        "--config",
    ];
    let mut out = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--image" {
            let _ = args.next();
            continue;
        }
        if arg.starts_with("--image=") {
            continue;
        }
        if PATH_FLAGS.contains(&arg.as_str()) {
            let _ = args.next();
            continue;
        }
        if PATH_FLAGS
            .iter()
            .any(|flag| arg.starts_with(&format!("{flag}=")))
        {
            continue;
        }
        out.push(arg);
    }
    out
}

fn probe_instance(instance: &str, root: &Path) -> Result<()> {
    let exe = env::current_exe().context("读不到当前 phprt 可执行文件")?;
    let output = Command::new(exe)
        .arg("__container-exec")
        .arg("--instance")
        .arg(instance)
        .arg("--workdir")
        .arg(root)
        .arg("--env")
        .arg(format!("{IN_CONTAINER}=1"))
        .arg("--")
        .arg("/phprt")
        .arg("--version")
        .output()
        .context("在构建实例里运行 phprt 探测失败")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "phprt 在构建镜像里跑不起来。\n{stderr}\n\
             换一个 glibc 不低于宿主、架构相同的镜像。"
        );
    }
    Ok(())
}

fn mount_specs(cli: &Cli, recipes: &Path, root: &Path) -> Vec<MountSpec> {
    let recipes = std::fs::canonicalize(recipes).unwrap_or_else(|_| recipes.to_path_buf());
    let mut mounts = vec![
        MountSpec {
            source: root.to_path_buf(),
            destination: root.to_path_buf(),
            readonly: false,
        },
        MountSpec {
            source: recipes.clone(),
            destination: recipes.clone(),
            readonly: true,
        },
    ];
    let mut seen: BTreeSet<PathBuf> = mounts.iter().map(|m| m.destination.clone()).collect();
    let mut push = |source: PathBuf, readonly: bool| {
        if !source.exists() || seen.contains(&source) {
            return;
        }
        seen.insert(source.clone());
        mounts.push(MountSpec {
            source: source.clone(),
            destination: source,
            readonly,
        });
    };
    push(layout::downloads_dir(root), false);
    push(layout::internal_dir(root), false);
    push(resolve_dir(root, layout::build_dir_name()), false);
    if let Some(name) = layout::backup_dir_name() {
        push(resolve_dir(root, name), false);
    }
    let config = effective_config_path(cli);
    if let Ok(config) = std::fs::canonicalize(&config) {
        push(config, true);
    }
    mounts
}

fn resolve_dir(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn resolve_target(target: &str, recipes: &Path) -> Result<(String, String)> {
    let (product, requested) = target
        .split_once('@')
        .ok_or_else(|| anyhow!("需要 产品@系列，例如 php@8.5"))?;
    let catalog = Catalog::load(&recipes.join("catalog.toml"))
        .with_context(|| format!("加载 {}", recipes.join("catalog.toml").display()))?;
    let series = catalog.find_product_series(product, requested)?;
    Ok((product.to_string(), series))
}

fn seed_default_components(
    project: &mut ProjectState,
    recipes: &Path,
    product: &str,
    series: &str,
) -> Result<()> {
    let path = recipes
        .join("bundles")
        .join(product)
        .join(format!("{series}.toml"));
    let bundle = Bundle::load(&path)?;
    bundle.check_product(product)?;
    for name in bundle.default_components.keys() {
        project
            .components
            .insert(name.clone(), ComponentState::default());
    }
    project.active = true;
    Ok(())
}

fn resolve_or_pull(
    store: &Store,
    query: &str,
    offline: bool,
) -> Result<phprt_buildimages::ImageInfo> {
    match store.resolve(query) {
        Ok(image) => Ok(image),
        Err(err) => {
            if normalize_digest(query).is_some() {
                return Err(err);
            }
            if offline {
                bail!("--offline 下镜像不在本地缓存：{query}");
            }
            if !confirm(&format!("本地没有镜像 {query}，现在拉取吗?"))? {
                bail!("镜像 {query} 不在本地。先执行：\n  phprt buildimages add {query}");
            }
            store.add_image(query, None, false)
        }
    }
}

fn confirm(question: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    print!("{question} [y/N] ");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let answer = line.trim();
    Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
}

pub fn scheme_text(project: &ProjectState, command: String) -> String {
    match project.build.as_ref().filter(|build| build.is_oci()) {
        Some(build) => {
            let mut out = String::new();
            if !build.image_ref.is_empty() {
                out.push_str(&format!(
                    "# 目标机器先准备镜像：phprt buildimages add {}\n",
                    build.image_ref
                ));
            }
            out.push_str(&format!("{command} --image {}\n", build.image));
            out
        }
        None => format!("{command}\n"),
    }
}

fn referenced_digests(root: &Path) -> Vec<String> {
    let dir = layout::internal_dir(root);
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.join("state.json").is_file() {
            continue;
        }
        let Ok(text) = fs::read_to_string(path.join("state.json")) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if let Some(image) = value
            .get("build")
            .and_then(|build| build.get("image"))
            .and_then(|image| image.as_str())
        {
            out.push(image.to_string());
        }
    }
    out
}

fn transform_install(command: &str, pm: &str) -> String {
    let mut command = command.to_string();
    if let Some(rest) = command.strip_prefix("sudo ") {
        command = rest.to_string();
    }
    if pm == "apt" {
        command = format!("apt-get update && {command}");
    }
    if !command.contains(" -y ") && !command.contains(" --yes") {
        command = command.replacen(" install ", " install -y ", 1);
    }
    command
}

/// 容器内缺构建命令时，按 `host_commands` 的 install 命令装包。只在
/// `PHPRT_IN_CONTAINER=1` 且非 offline 的路径上调用。
pub fn install_missing(report: &host_commands::Report) -> Result<()> {
    if report.install.is_empty() {
        bail!("{}", report.error_message());
    }
    let command = transform_install(report.install.trim(), &report.pm);
    println!("容器里安装构建命令：{command}");
    let status = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .status()
        .with_context(|| format!("执行 {command} 失败"))?;
    if !status.success() {
        bail!("装包失败：{command}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_commands_become_non_interactive() {
        assert_eq!(
            transform_install("sudo dnf install cmake", "dnf"),
            "dnf install -y cmake"
        );
        assert_eq!(
            transform_install("sudo apt-get install perl", "apt"),
            "apt-get update && apt-get install -y perl"
        );
        assert_eq!(
            transform_install("dnf install -y gcc", "dnf"),
            "dnf install -y gcc"
        );
    }

    #[test]
    fn forwarded_args_drop_image_and_absolute_paths() {
        let args = vec![
            "--root".to_string(),
            "rel".to_string(),
            "--images=x".to_string(),
            "install".to_string(),
            "php@8.5".to_string(),
            "--image".to_string(),
            "fedora43".to_string(),
            "--recipes=/r".to_string(),
            "--parallel".to_string(),
            "4".to_string(),
        ];
        assert_eq!(
            filter_forward_args(args),
            vec!["--images=x", "install", "php@8.5", "--parallel", "4"]
        );
    }

    #[test]
    fn scheme_text_exports_image_digest() {
        let project = ProjectState {
            build: Some(BuildState {
                mode: "oci".to_string(),
                image: "sha256:aa".to_string(),
                image_ref: "quay.io/fedora/fedora:43".to_string(),
                runtime: RUNTIME.to_string(),
                ..BuildState::default()
            }),
            ..ProjectState::default()
        };
        let text = scheme_text(&project, "phprt install php@8.5".to_string());
        assert!(text.contains("phprt buildimages add quay.io/fedora/fedora:43"));
        assert!(text.contains("--image sha256:aa"));
        assert_eq!(signature_key(&project), "oci:sha256:aa");
        assert_eq!(signature_key(&ProjectState::default()), "host");
    }
}
