//! 构建环境调度：用户级镜像缓存、实例创建、命令转发、容器内装包。
//!
//! 这里只做 phprt 入口层的判断和转发；镜像/容器本身由 `phprt-buildimages`
//! 负责。容器内的 phprt 通过 `PHPRT_IN_CONTAINER=1` 识别自己，不再递归转发。

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use phprt_buildimages::{
    ImageInfo, InstanceInfo, InstanceLock, InstanceRequest, MountSpec, Store, dir_size,
    instance_id, is_running, legacy_instance_name, lock_instance, normalize_digest, project_hash,
    short_digest, try_lock_instance, validate_instance_name,
};

use crate::bundle::Bundle;
use crate::catalog::Catalog;
use crate::host_commands;
use crate::image_family;
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
            BuildimagesCmd::Use { .. }
                | BuildimagesCmd::Rm { .. }
                | BuildimagesCmd::RmInstance { .. }
        )
    {
        bail!("buildimages use / rm / rm-instance 只支持本地模式");
    }
    let store = Store::open()?;
    let root = canonical_or(&cli.root);
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
            let families = image_family::load(&recipes.join("images.toml"))?;
            if let Some(family) = image_family::detect(&families, &image.rootfs())? {
                image_family::print_details(family);
            }
            println!();
            println!("下一步：");
            println!(
                "  phprt install <产品>@<系列> --image {} --instance-name <名字>",
                short_digest(&image.digest)
            );
            println!(
                "  或 phprt buildimages use <产品>@<系列> --image {} --instance-name <名字>",
                short_digest(&image.digest)
            );
            println!("  phprt buildimages list");
        }
        BuildimagesCmd::List => {
            let images = store.list()?;
            let instances = store.list_instances()?;
            let refs = all_state_refs(&root, &store);
            let mut by_digest: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for instance in &instances {
                by_digest
                    .entry(instance.image.clone())
                    .or_default()
                    .push(instance.name.clone());
            }

            if images.is_empty() {
                println!("本地还没有构建镜像。先执行 phprt buildimages add <镜像引用>");
            } else {
                println!(
                    "{:<14} {:<20} {:<10} {:<12} {:<12} {:>8}  {:<24}  refs",
                    "digest", "alias", "family", "platform", "created", "size", "instances"
                );
                let families = image_family::load(&recipes.join("images.toml"))?;
                for image in images {
                    let family = image_family::name_for(&families, &image.rootfs())?;
                    let users = by_digest
                        .get(&image.digest)
                        .map(|names| names.join(","))
                        .unwrap_or_default();
                    println!(
                        "{:<14} {:<20} {:<10} {:<12} {:<12} {:>7.1}M  {:<24}  {}",
                        short_digest(&image.digest),
                        image.aliases.join(","),
                        family,
                        image.platform,
                        image.created,
                        image.size as f64 / 1024.0 / 1024.0,
                        users,
                        image.refs.join(", ")
                    );
                }
            }

            println!();
            if instances.is_empty() {
                println!("本地还没有构建实例。");
            } else {
                println!(
                    "{:<28} {:<14} {:<36} {:<24} {:>8}  state",
                    "name", "image", "owner", "used_by", "size"
                );
                for instance in instances {
                    let used = used_by(&refs, &instance);
                    let owner = if instance.owner.is_empty() {
                        "-".to_string()
                    } else if Path::new(&instance.owner).is_dir() {
                        instance.owner.clone()
                    } else {
                        format!("{} (missing)", instance.owner)
                    };
                    let state = if is_running(&instance.id) {
                        "running"
                    } else {
                        "stopped"
                    };
                    println!(
                        "{:<28} {:<14} {:<36} {:<24} {:>7.1}M  {}",
                        instance.name,
                        short_digest(&instance.image),
                        owner,
                        used.join(","),
                        dir_size(&instance.rootfs) as f64 / 1024.0 / 1024.0,
                        state
                    );
                }
            }
        }
        BuildimagesCmd::Use {
            target,
            image,
            instance,
            instance_name,
            host,
            force,
        } => {
            let choices = *host as u8 + image.is_some() as u8 + instance.is_some() as u8;
            if choices != 1 {
                bail!("use 需要 --image / --instance / --host 三选一");
            }
            if instance_name.is_some() && image.is_none() {
                bail!("--instance-name 需要和 --image 一起用；使用已有实例用 --instance <名字>");
            }
            let (product, series) = resolve_target(target, recipes)?;
            let (selection, family) = if *host {
                (None, None)
            } else if let Some(name) = instance {
                let info = store.resolve_instance(name)?;
                let family = family_for_digest(&store, recipes, &info.image)?;
                (
                    Some(BuildState {
                        mode: "oci".to_string(),
                        image: info.image,
                        image_ref: info.image_ref,
                        runtime: RUNTIME.to_string(),
                        instance: info.name,
                    }),
                    family,
                )
            } else {
                let query = image.as_deref().expect("clap 已保证");
                let info = resolve_or_pull(&store, query, false)?;
                let name = if let Some(name) = instance_name {
                    validate_instance_name(name)?;
                    name.clone()
                } else {
                    default_instance_name(&product, &series, &root)
                };
                validate_instance_name(&name)?;
                if let Some(existing) = store.find_instance(&name)? {
                    if existing.image != info.digest {
                        bail!(
                            "实例 {name} 已绑定镜像 {}，不能换成 {}。\n  \
                             换镜像请用另一个 --instance-name",
                            short_digest(&existing.image),
                            short_digest(&info.digest)
                        );
                    }
                } else if store.resolve(&name).is_ok() {
                    bail!("{name} 已经是镜像别名；实例名请换一个，或改用 --image {name}");
                }
                let family = family_for_digest(&store, recipes, &info.digest)?;
                (
                    Some(BuildState {
                        mode: "oci".to_string(),
                        image: info.digest,
                        image_ref: info.reference,
                        runtime: RUNTIME.to_string(),
                        instance: name,
                    }),
                    family,
                )
            };

            let state_file = state::state_path(&root, &product, &series);
            let mut project = ProjectState::load(&state_file)?;
            project.check(&product, &series)?;
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
            if let Some(build) = &project.build
                && !build.instance.is_empty()
            {
                println!("实例      {}", build.instance);
            }
            if let Some(family) = &family {
                image_family::print_details(family);
            }
            println!("已写入 {}", state_file.display());
            println!("下一次 phprt install {}@{} 生效", product, series);
        }
        BuildimagesCmd::Rm { image, force } => {
            let image = store.resolve(image)?;
            let users: Vec<String> = store
                .list_instances()?
                .into_iter()
                .filter(|instance| instance.image == image.digest)
                .map(|instance| format!("{}（{}）", instance.name, instance.id))
                .collect();
            if !users.is_empty() {
                bail!(
                    "镜像 {} 仍被以下实例使用：\n  {}\n\
                     先执行 phprt buildimages rm-instance <名字>，再删除镜像",
                    short_digest(&image.digest),
                    users.join("\n  ")
                );
            }
            let refs = refs_for_digest(&root, &store, &image.digest);
            if !refs.is_empty() && !force {
                bail!(
                    "镜像 {} 仍被以下 state 引用：\n  {}\n\
                     加 --force 才删除；下次 install 会重新拉取",
                    short_digest(&image.digest),
                    refs.iter()
                        .map(StateRef::label)
                        .collect::<Vec<_>>()
                        .join("\n  ")
                );
            }
            if !refs.is_empty() {
                println!("警告：以下 state 仍引用该 digest，下次 install 会重新拉取：");
                for item in &refs {
                    println!("  {}", item.label());
                }
            }
            store.remove_image(&image)?;
            println!("已删除镜像 {}", short_digest(&image.digest));
        }
        BuildimagesCmd::RmInstance { name, force } => {
            let instance = store.resolve_instance(name)?;
            let refs = refs_for_instance(&root, &store, &instance);
            if !refs.is_empty() && !force {
                bail!(
                    "实例 {} 仍被以下 state 引用：\n  {}\n\
                     加 --force 才删除；下次命令会按镜像重建该实例",
                    instance.name,
                    refs.iter()
                        .map(StateRef::label)
                        .collect::<Vec<_>>()
                        .join("\n  ")
                );
            }
            if !refs.is_empty() {
                println!("警告：以下 state 仍指向该实例，下次命令会按镜像重建：");
                for item in &refs {
                    println!("  {}", item.label());
                }
            }
            let Some(_lock) = try_lock_instance(&store, &instance.id)? else {
                bail!(
                    "实例 {} 正在被另一个 phprt 使用；等命令结束再删除",
                    instance.name
                );
            };
            let image = instance.image.clone();
            store.remove_instance(&instance.id)?;
            println!("已删除实例 {}", instance.name);
            let remaining = store
                .list_instances()?
                .into_iter()
                .filter(|item| item.image == image)
                .count();
            if remaining == 0 {
                println!("镜像 {} 现在可以删除", short_digest(&image));
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
    let (target, cli_image, cli_instance, cli_instance_name, offline, plan, plan_json, force) =
        match &cli.cmd {
            Cmd::Plan {
                target,
                image,
                instance,
                instance_name,
                json,
                ..
            } => (
                target.clone(),
                image.clone(),
                instance.clone(),
                instance_name.clone(),
                false,
                true,
                *json,
                false,
            ),
            Cmd::Install {
                target,
                image,
                instance,
                instance_name,
                download,
                force,
                ..
            } => {
                let target = target.first().cloned().unwrap_or_default();
                (
                    target,
                    image.clone(),
                    instance.clone(),
                    instance_name.clone(),
                    download.offline,
                    false,
                    false,
                    *force,
                )
            }
            Cmd::Update {
                target,
                image,
                instance,
                instance_name,
                download,
                ..
            } => (
                target.first().cloned().unwrap_or_default(),
                image.clone(),
                instance.clone(),
                instance_name.clone(),
                download.offline,
                false,
                false,
                false,
            ),
            Cmd::Add {
                target,
                image,
                instance,
                instance_name,
                download,
                ..
            } => (
                target.clone(),
                image.clone(),
                instance.clone(),
                instance_name.clone(),
                download.offline,
                false,
                false,
                false,
            ),
            Cmd::Remove {
                target, download, ..
            } => (
                target.clone(),
                None,
                None,
                None,
                download.offline,
                false,
                false,
                false,
            ),
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
    if recorded.is_none()
        && cli_image.is_none()
        && cli_instance.is_none()
        && cli_instance_name.is_none()
    {
        return Ok(false);
    }
    if layout::is_global() {
        bail!("构建镜像只支持本地模式，不能和 --global --prefix 一起用");
    }

    let store = Store::open()?;
    let root = std::fs::canonicalize(&cli.root)
        .with_context(|| format!("读不到项目根 {}", cli.root.display()))?;
    let Some(choice) = choose_build(
        &store,
        &root,
        &product,
        &series,
        recorded.as_ref(),
        cli_image.as_deref(),
        cli_instance.as_deref(),
        cli_instance_name.as_deref(),
        offline,
    )?
    else {
        return Ok(false);
    };

    ensure_instance_image(&store, &choice, offline)?;

    if !plan {
        let current = project.build.as_ref().map(BuildState::signature_key);
        let wanted = format!("oci:{}", choice.image);
        if !project.installed_version.is_empty()
            && current.as_deref() != Some(wanted.as_str())
            && !force
        {
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
        project.build = Some(BuildState {
            mode: "oci".to_string(),
            image: choice.image.clone(),
            image_ref: choice.image_ref.clone(),
            runtime: RUNTIME.to_string(),
            instance: choice.name.clone(),
        });
        project.save(&state_file)?;
    }

    let request = InstanceRequest {
        id: choice.id.clone(),
        name: choice.name.clone(),
        image: choice.image.clone(),
        image_ref: choice.image_ref.clone(),
        project_root: root.clone(),
        mounts: mount_specs(cli, recipes, &root),
        executable: env::current_exe().context("读不到当前 phprt 可执行文件")?,
    };
    let _lock = acquire_instance_lock(&store, &request.id, &request.name)?;
    let instance = phprt_buildimages::ensure_instance(&store, &request)?;
    let families = image_family::load(&recipes.join("images.toml"))?;
    if let Some(family) = image_family::detect(&families, &instance.rootfs)? {
        image_family::apply(family, &instance.rootfs)?;
        if plan && !plan_json {
            println!("镜像家族  {}", image_family::summary(family));
        }
    }
    probe_instance(&instance.id, &root)?;

    if !plan {
        let mut build = project.build.clone().unwrap_or_default();
        build.image = instance.image.clone();
        build.image_ref = instance.image_ref.clone();
        build.instance = instance.name.clone();
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

struct Choice {
    id: String,
    name: String,
    image: String,
    image_ref: String,
}

#[allow(clippy::too_many_arguments)]
fn choose_build(
    store: &Store,
    root: &Path,
    product: &str,
    series: &str,
    recorded: Option<&BuildState>,
    cli_image: Option<&str>,
    cli_instance: Option<&str>,
    cli_instance_name: Option<&str>,
    offline: bool,
) -> Result<Option<Choice>> {
    if cli_image.is_none()
        && cli_instance.is_none()
        && cli_instance_name.is_none()
        && recorded.is_none()
    {
        return Ok(None);
    }
    let default_name = default_instance_name(product, series, root);

    if let Some(query) = cli_instance {
        let instance = store.resolve_instance(query)?;
        if let Some(image) = cli_image {
            let info = resolve_or_pull(store, image, offline)?;
            if info.digest != instance.image {
                bail!(image_mismatch(
                    &instance.name,
                    &instance.image,
                    &info.digest,
                    Some(image)
                ));
            }
        }
        return Ok(Some(Choice {
            id: instance.id,
            name: instance.name,
            image: instance.image,
            image_ref: instance.image_ref,
        }));
    }

    let mut existing = None;
    let name = if let Some(name) = cli_instance_name {
        validate_instance_name(name)?;
        name.to_owned()
    } else if let Some(recorded) = recorded {
        if recorded.instance.is_empty() {
            default_name
        } else {
            match store.find_instance(&recorded.instance)? {
                Some(instance) => {
                    let name = instance.name.clone();
                    existing = Some(instance);
                    name
                }
                None if legacy_instance_name(&recorded.instance).is_some() => default_name,
                None => recorded.instance.clone(),
            }
        }
    } else {
        default_name
    };
    validate_instance_name(&name)?;

    if existing.is_none() {
        existing = store.find_instance(&name)?;
    }
    if let Some(instance) = existing {
        if let Some(image) = cli_image {
            let info = resolve_or_pull(store, image, offline)?;
            if info.digest != instance.image {
                bail!(image_mismatch(
                    &instance.name,
                    &instance.image,
                    &info.digest,
                    Some(image)
                ));
            }
        }
        return Ok(Some(Choice {
            id: instance.id,
            name: instance.name,
            image: instance.image,
            image_ref: instance.image_ref,
        }));
    }

    if store.resolve(&name).is_ok() {
        bail!("{name} 已经是镜像别名；实例名请换一个，或改用 --image {name}");
    }

    let query = cli_image.map(str::to_owned).or_else(|| {
        recorded.map(|recorded| {
            if recorded.image_ref.is_empty() {
                recorded.image.clone()
            } else {
                recorded.image_ref.clone()
            }
        })
    });
    let query = query.ok_or_else(|| {
        anyhow!(
            "实例 {name} 不存在；新建实例请给 --image <镜像>，\
             或先 phprt buildimages add <镜像>"
        )
    })?;
    let info = resolve_or_pull(store, &query, offline)?;
    Ok(Some(Choice {
        id: instance_id(&name),
        name,
        image: info.digest,
        image_ref: info.reference,
    }))
}

fn image_mismatch(name: &str, current: &str, wanted: &str, query: Option<&str>) -> String {
    let hint = query
        .map(|query| format!("\n  换镜像请用 --instance-name <新名字> --image {query}"))
        .unwrap_or_default();
    format!(
        "实例 {name} 已绑定镜像 {}，不能换成 {}。{hint}",
        short_digest(current),
        short_digest(wanted)
    )
}

/// 实例 rootfs 丢失、需要从镜像重建时，先把镜像准备好。
fn ensure_instance_image(store: &Store, choice: &Choice, offline: bool) -> Result<()> {
    let rootfs = store.instance_dir(&choice.id).join("bundle/rootfs");
    if rootfs.join("usr").is_dir() || store.resolve(&choice.image).is_ok() {
        return Ok(());
    }
    if choice.image_ref.is_empty() {
        bail!(
            "实例 {} 的 rootfs 已丢失，镜像 {} 也不在本地；\
             先执行 phprt buildimages add <镜像引用> 再重试",
            choice.name,
            short_digest(&choice.image)
        );
    }
    let info = resolve_or_pull(store, &choice.image_ref, offline)?;
    if info.digest != choice.image {
        bail!(
            "实例 {} 需要镜像 {} 才能重建，但 {} 现在指向 {}；\n  \
             先执行 phprt buildimages add {}（用原 digest）",
            choice.name,
            short_digest(&choice.image),
            choice.image_ref,
            short_digest(&info.digest),
            choice.image
        );
    }
    Ok(())
}

fn acquire_instance_lock(store: &Store, id: &str, name: &str) -> Result<InstanceLock> {
    match try_lock_instance(store, id)? {
        Some(lock) => Ok(lock),
        None => {
            println!("等待实例 {name} 空闲...");
            lock_instance(store, id)
        }
    }
}

fn default_instance_name(product: &str, series: &str, root: &Path) -> String {
    format!("{product}-{series}-{}", project_hash(root))
}

fn canonical_or(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn family_for_digest(
    store: &Store,
    recipes: &Path,
    digest: &str,
) -> Result<Option<image_family::Family>> {
    let rootfs = store.image_dir(digest).join("rootfs");
    if !rootfs.is_dir() {
        return Ok(None);
    }
    let families = image_family::load(&recipes.join("images.toml"))?;
    Ok(image_family::detect(&families, &rootfs)?.cloned())
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
        if arg == "--image" || arg == "--instance" || arg == "--instance-name" {
            let _ = args.next();
            continue;
        }
        if arg.starts_with("--image=")
            || arg.starts_with("--instance=")
            || arg.starts_with("--instance-name=")
        {
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

fn resolve_or_pull(store: &Store, query: &str, offline: bool) -> Result<ImageInfo> {
    match store.resolve(query) {
        Ok(image) => Ok(image),
        Err(err) => {
            let prefix = query.strip_prefix("sha256:").unwrap_or(query);
            let is_digest_prefix = prefix.len() >= 8
                && prefix.len() < 64
                && prefix.chars().all(|c| c.is_ascii_hexdigit());
            if normalize_digest(query).is_some() || is_digest_prefix {
                return Err(err);
            }
            if offline {
                bail!("--offline 下镜像不在本地缓存：{query}");
            }
            println!("本地没有镜像 {query}，开始拉取...");
            store.add_image(query, None, false)
        }
    }
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

#[derive(Debug, Clone)]
struct StateRef {
    project: PathBuf,
    product: String,
    series: String,
    digest: String,
    instance: String,
}

impl StateRef {
    fn label(&self) -> String {
        let target = if self.product.is_empty() && self.series.is_empty() {
            "?".to_string()
        } else {
            format!("{}@{}", self.product, self.series)
        };
        format!("{} ({target})", self.project.display())
    }
}

fn scan_project_states(project: &Path, internal: &Path) -> Vec<StateRef> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(internal) else {
        return out;
    };
    for entry in entries.flatten() {
        let state = entry.path().join("state.json");
        let Ok(text) = fs::read_to_string(&state) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(build) = value.get("build") else {
            continue;
        };
        let digest = build
            .get("image")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .to_string();
        if digest.is_empty() {
            continue;
        }
        out.push(StateRef {
            project: project.to_path_buf(),
            product: value
                .get("product")
                .and_then(|item| item.as_str())
                .unwrap_or_default()
                .to_string(),
            series: value
                .get("series")
                .and_then(|item| item.as_str())
                .unwrap_or_default()
                .to_string(),
            digest,
            instance: build
                .get("instance")
                .and_then(|item| item.as_str())
                .unwrap_or_default()
                .to_string(),
        });
    }
    out
}

fn all_state_refs(root: &Path, store: &Store) -> Vec<StateRef> {
    let mut refs = scan_project_states(root, &layout::internal_dir(root));
    let mut seen = BTreeSet::new();
    seen.insert(root.to_path_buf());
    for instance in store.list_instances().unwrap_or_default() {
        for project in instance.projects {
            let project = PathBuf::from(project);
            if !seen.insert(project.clone()) {
                continue;
            }
            refs.extend(scan_project_states(&project, &project.join(".phprt")));
        }
    }
    refs
}

fn used_by(refs: &[StateRef], instance: &InstanceInfo) -> Vec<String> {
    let mut out = BTreeSet::new();
    for item in refs {
        if item.instance != instance.name && item.instance != instance.id {
            continue;
        }
        if item.product.is_empty() && item.series.is_empty() {
            continue;
        }
        out.insert(format!("{}@{}", item.product, item.series));
    }
    out.into_iter().collect()
}

fn refs_for_instance(root: &Path, store: &Store, instance: &InstanceInfo) -> Vec<StateRef> {
    all_state_refs(root, store)
        .into_iter()
        .filter(|item| item.instance == instance.name || item.instance == instance.id)
        .collect()
}

fn refs_for_digest(root: &Path, store: &Store, digest: &str) -> Vec<StateRef> {
    all_state_refs(root, store)
        .into_iter()
        .filter(|item| item.digest == digest)
        .collect()
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
    fn forwarded_args_drop_image_instance_and_absolute_paths() {
        let args = vec![
            "--root".to_string(),
            "rel".to_string(),
            "--images=x".to_string(),
            "install".to_string(),
            "php@8.5".to_string(),
            "--image".to_string(),
            "fedora43".to_string(),
            "--instance".to_string(),
            "fedora43mybuild_os".to_string(),
            "--instance-name=php-build".to_string(),
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
                instance: "fedora43mybuild_os".to_string(),
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
