mod actions;
mod audit;
mod bind;
mod build;
mod bundle;
mod catalog;
mod catalog_publish;
mod catalog_remote;
mod config;
mod host_commands;
mod layout;
mod lock;
mod manage;
mod marker;
mod plan;
mod recipe;
mod render;
mod sources;
mod state;
mod targets;
mod toolchain;
mod tools;
mod version;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};

use crate::bundle::Bundle;
use crate::catalog::Catalog;
use crate::recipe::{Kind, RawComponent, RecipeMeta, Registry, Selection, Strategy, Target};
use crate::state::ProjectState;
use phprt_ui::Mode;

#[derive(Parser)]
#[command(
    name = "phprt",
    version,
    about = "phprt v3：Starlark 写 recipe，Rust 负责求解、下载、解压和构建"
)]
struct Cli {
    /// 配置文件；默认 <root>/phprt.toml，空值按默认处理
    #[arg(short = 'c', long = "config", global = true, value_parser = path_value)]
    config: Option<PathBuf>,

    /// recipe 目录；空或未指定时用 phprt.toml 的 recipes，再退回 ./recipes
    #[arg(long, global = true, value_parser = path_value)]
    recipes: Option<PathBuf>,

    /// 项目根目录；本地模式的工具目录默认 <root>/.phprt
    #[arg(long, default_value = ".", global = true)]
    root: PathBuf,

    /// 本地模式的工具目录；覆盖 local_internal_dir，可绝对或相对
    #[arg(long = "internal-dir", global = true, value_parser = path_value)]
    internal_dir: Option<PathBuf>,

    /// 解压目录；覆盖 local_build_dir / global_build_dir，可绝对或相对
    #[arg(long = "build-dir", global = true, value_parser = path_value)]
    build_dir: Option<PathBuf>,

    /// 备份目录；覆盖 local_backup_dir / global_backup_dir，可绝对或相对
    #[arg(long = "backup-dir", global = true, value_parser = path_value)]
    backup_dir: Option<PathBuf>,

    /// 编译并发数；覆盖 phprt.toml 的 parallel，默认 CPU 数 - 1（最少 1）。shell 的 MAKEFLAGS 无效
    #[arg(long, global = true, value_name = "N")]
    parallel: Option<usize>,

    /// 全局模式：把产品直接装到 --prefix，不在项目里生成安装树
    #[arg(long, global = true)]
    global: bool,

    /// 全局安装目录；只在 --global 时使用
    #[arg(long, global = true, value_parser = path_value)]
    prefix: Option<PathBuf>,

    /// 下载缓存目录；两种模式都可用，覆盖 phprt.toml 的 download_dir
    #[arg(long = "download-dir", global = true, value_parser = path_value)]
    download_dir: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Clone)]
enum Cmd {
    /// 列出已安装的产品；带 产品@系列 时列出该产品的组件
    List {
        /// 产品@系列，如 php@8.5
        target: Option<String>,
        /// 只列已安装
        #[arg(long, conflicts_with = "pending")]
        installed: bool,
        /// 只列已加入方案但未安装
        #[arg(long)]
        pending: bool,
    },
    /// 打印依赖关系
    Graph {
        /// 产品@系列，如 php@8.5
        target: String,
    },
    /// 求解并打印构建计划（只读）
    Plan {
        /// 产品@系列，如 php@8.5
        target: String,
        /// 目标，如 product:php@8.5.12 / component:curl / dep:openssl
        goal: Option<String>,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
        /// 把已有组件和依赖锁到 state 记录的实际补丁
        #[arg(long)]
        lock: bool,
        #[command(flatten)]
        components: ComponentArgs,
        #[command(flatten)]
        targets: TargetArgs,
    },
    /// 打印一条可复制执行的重建命令
    Scheme {
        /// 产品@系列，如 php@8.5
        target: String,
        /// 导出产品补丁、实际版本线和精确补丁
        #[arg(long)]
        lock: bool,
    },
    /// 下载、解压、编译并安装；只接受一个产品
    Install {
        /// 产品@系列，如 php@8.5；后面可以再跟一份完整方案列表
        target: Vec<String>,
        #[command(flatten)]
        components: ComponentArgs,
        #[command(flatten)]
        targets: TargetArgs,
        #[command(flatten)]
        download: DownloadArgs,
        /// 已装的补丁先改名备份再全新安装；补丁不同时直接装目标补丁
        #[arg(long)]
        force: bool,
        /// 本次不备份 PHP 扩展的 ini
        #[arg(long = "no-backup-php-ext-ini")]
        no_backup_php_ext_ini: bool,
        /// 把已有组件和依赖锁到 state 记录的实际补丁
        #[arg(long)]
        lock: bool,
    },
    /// 更新：重新从 catalog 解析该产品线的最新补丁并构建
    Update {
        /// 产品@系列，如 php@8.5
        target: Vec<String>,
        #[command(flatten)]
        components: ComponentArgs,
        #[command(flatten)]
        targets: TargetArgs,
        #[command(flatten)]
        download: DownloadArgs,
        /// 本次不备份 PHP 扩展的 ini
        #[arg(long = "no-backup-php-ext-ini")]
        no_backup_php_ext_ini: bool,
    },
    /// 把组件加入方案并立即增量构建
    Add {
        /// 产品@系列，如 php@8.5
        target: String,
        /// 组件 spec，如 swoole / 'swoole=mode:shared,version:6.2'
        component: String,
        /// 直接绑定依赖库的版本线：NAME=LINE，可重复
        #[arg(long = "dep", value_name = "NAME=LINE")]
        dep: Vec<String>,
        #[command(flatten)]
        download: DownloadArgs,
        /// 跳过磁盘一致性检查
        #[arg(long)]
        force: bool,
        /// 本次不备份 PHP 扩展的 ini
        #[arg(long = "no-backup-php-ext-ini")]
        no_backup_php_ext_ini: bool,
        /// 把已有组件和依赖锁到 state 记录的实际补丁
        #[arg(long)]
        lock: bool,
    },
    /// 把组件移出方案并删掉它的产物
    Remove {
        /// 产品@系列，如 php@8.5
        target: String,
        /// 组件名
        name: String,
        #[command(flatten)]
        download: DownloadArgs,
        /// 跳过磁盘一致性检查（缺文件时只更新方案）
        #[arg(long)]
        force: bool,
        /// 本次不备份 PHP 扩展的 ini
        #[arg(long = "no-backup-php-ext-ini")]
        no_backup_php_ext_ini: bool,
        /// 把已有组件和依赖锁到 state 记录的实际补丁
        #[arg(long)]
        lock: bool,
    },
    /// 对照上游选项清单，检查 recipe 是否漏掉或静默忽略了开关
    Audit {
        /// 产品@系列，如 php@8.5
        target: String,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
    },
    /// 维护 recipes/catalog.toml：查上游、写回、发布组件分片
    Catalog {
        #[command(subcommand)]
        command: CatalogCmd,
    },
}

#[derive(Subcommand, Clone)]
enum CatalogCmd {
    /// 查询上游最新补丁，写进 catalog.toml（写前备份）
    Update {
        /// 只打印计划，不写文件
        #[arg(long)]
        dry_run: bool,
        /// 回填已有条目缺失的 sha256（默认不做，普通使用不用加）
        #[arg(long = "fill-sha256")]
        fill_sha256: bool,
        /// 只更新这些组件；空表示全部（没有 upstream 脚本的跳过）
        components: Vec<String>,
    },
    /// 把 catalog.toml 拆成每个组件一个分片，供用户在线获取
    Publish {
        /// 输出目录；实际写到 <out>/latest/components/
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Args, Clone, Default)]
struct ComponentArgs {
    /// 组件选择：名字[=mode:方式][,dep:依赖[feature,...]][,feature:[名字,...]][,version:线]；可重复
    #[arg(long = "component", value_name = "SPEC")]
    component: Vec<String>,
    /// 直接绑定依赖库的版本线：NAME=LINE，可重复
    #[arg(long = "dep", value_name = "NAME=LINE")]
    dep: Vec<String>,
    /// 禁用产品的默认组件；可重复
    #[arg(long = "disable-default-component", value_name = "NAME")]
    disable_default_component: Vec<String>,
}

#[derive(Args, Clone, Default)]
struct TargetArgs {
    /// 构建目标：cli、cli,fpm、cli,fpm=user:www-data,group:www-data；可重复
    #[arg(long = "targets", value_name = "SPEC")]
    targets: Vec<String>,
}

#[derive(Args, Clone)]
struct DownloadArgs {
    /// 只下载源码，不解压、不写状态
    #[arg(long)]
    downloads: bool,
    /// 文件级并发数
    #[arg(long = "download-jobs", default_value_t = 10, value_name = "N")]
    download_jobs: usize,
    /// 同一站点并发数
    #[arg(long = "download-host-jobs", default_value_t = 2, value_name = "N")]
    download_host_jobs: usize,
    /// 单文件分片数
    #[arg(long = "connections", default_value_t = 4, value_name = "N")]
    connections: usize,
    /// 单个请求总超时（秒）
    #[arg(long = "timeout", default_value_t = 900, value_name = "SECONDS")]
    timeout: u64,
    /// 本次下载使用的代理
    #[arg(long, value_name = "URL")]
    proxy: Option<String>,
    /// 只用缓存，不联网
    #[arg(long)]
    offline: bool,
    /// 跳过询问，直接获取最新 catalog；失败按错误处理
    #[arg(long, conflicts_with_all = ["no_refresh", "offline"])]
    refresh: bool,
    /// 跳过询问，不获取 catalog
    #[arg(long = "no-refresh")]
    no_refresh: bool,
    /// 进度显示方式
    #[arg(long, value_enum, default_value_t = Mode::Auto, value_name = "MODE")]
    ui: Mode,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let config_file = effective_config(&cli);
    if cli
        .config
        .as_deref()
        .is_some_and(|path| !path.as_os_str().is_empty())
        && !config_file.is_file()
    {
        bail!("配置不存在: {}", config_file.display());
    }
    let config = config::Config::load(&config_file)?;
    match (cli.global, cli.prefix.as_deref()) {
        (true, None) => bail!("--global 需要 --prefix，例如 --prefix /usr/local/php"),
        (false, Some(_)) => bail!("--prefix 只能和 --global 一起用"),
        _ => {}
    }
    if cli.global
        && cli
            .internal_dir
            .as_deref()
            .is_some_and(|path| !path.as_os_str().is_empty())
    {
        bail!("--internal-dir 只用于本地模式；全局模式用 --prefix");
    }
    if let Some(prefix) = &cli.prefix {
        layout::set_global_prefix(prefix.clone());
    }
    if layout::is_global() {
        let build = effective_global_dir(
            cli.build_dir.as_deref(),
            &config.global_build_dir_name()?,
            "~/phprtbuild",
            "global_build_dir",
        )?;
        let backup = effective_global_dir(
            cli.backup_dir.as_deref(),
            &config.global_backup_dir_name()?,
            "~/phprtbackup",
            "global_backup_dir",
        )?;
        layout::set_build_dir_name(&build);
        layout::set_backup_dir_name(&backup);
        if let Some(dir) = effective_download_dir(
            cli.download_dir.as_deref(),
            config.download_dir_name()?.as_deref(),
            &cli.root,
            true,
        )? {
            layout::set_download_dir(dir);
        }
    } else {
        let internal = effective_dir(
            cli.internal_dir.as_deref(),
            &config.local_internal_dir_name()?,
            layout::DEFAULT_INTERNAL_DIR,
            "local_internal_dir",
        )?;
        let build = effective_dir(
            cli.build_dir.as_deref(),
            &config.local_build_dir_name()?,
            layout::DEFAULT_BUILD_DIR,
            "local_build_dir",
        )?;
        layout::set_internal_dir_name(&internal);
        layout::set_build_dir_name(&build);
        if let Some(backup) = effective_dir_opt(
            cli.backup_dir.as_deref(),
            &config.local_backup_dir_name()?,
            "local_backup_dir",
        )? {
            layout::set_backup_dir_name(&backup);
        }
        if let Some(dir) = effective_download_dir(
            cli.download_dir.as_deref(),
            config.download_dir_name()?.as_deref(),
            &cli.root,
            false,
        )? {
            layout::set_download_dir(dir);
        }
    }
    let recipes = effective_recipes(cli.recipes.as_deref(), &cli.root, &config);

    if let Cmd::List {
        target: None,
        installed,
        pending,
    } = &cli.cmd
    {
        if *installed || *pending {
            bail!("--installed / --pending 需要指定产品，例如 phprt list php@8.5");
        }
        print_states(&cli.root)?;
        return Ok(());
    }

    // catalog 命令不带产品目标，不需要求解器。
    if let Cmd::Catalog { command } = &cli.cmd {
        match command {
            CatalogCmd::Update {
                dry_run,
                fill_sha256,
                components,
            } => {
                let backup = match layout::backup_dir_name() {
                    Some(name) => layout::resolve(&cli.root, name),
                    None => layout::internal_dir(&cli.root).join("backup"),
                };
                let report = phprt_upstream::run(phprt_upstream::Options {
                    recipes: recipes.clone(),
                    cache: layout::downloads_dir(&cli.root),
                    backup_dir: backup,
                    // 并发 / 分片 / 超时和 install 的默认值一致（docs/DOWNLOAD.md 5）。
                    download: phprt_download::Options {
                        jobs: 10,
                        host_jobs: 2,
                        connections: 4,
                        timeout: 900,
                        proxy: None,
                        github_proxy: config.github_proxy()?,
                        offline: false,
                        // 和 install 的默认一致：终端里画进度框，非终端逐行输出。
                        ui: Mode::Auto,
                    },
                    dry_run: *dry_run,
                    fill_sha256: *fill_sha256,
                    components: components.clone(),
                    validate: Box::new(|path| Catalog::load(path).map(|_| ())),
                })?;
                print!("{}", report.text());
                if report.partial_failure {
                    std::process::exit(3);
                }
            }
            CatalogCmd::Publish { out } => {
                let summary = catalog_publish::run(&recipes.join("catalog.toml"), out)?;
                println!(
                    "已发布 {} 个组件分片到 {}",
                    summary.components,
                    summary.out.display()
                );
                println!("数据时间 {}", summary.stamp);
            }
        }
        return Ok(());
    }

    let mut allow_online = true;
    loop {
        match run_body(&cli, &config, &recipes, allow_online)? {
            RunOutcome::Done => return Ok(()),
            RunOutcome::Retry => allow_online = false,
        }
    }
}

enum RunOutcome {
    Done,
    Retry,
}

fn run_body(
    cli: &Cli,
    config: &config::Config,
    recipes: &Path,
    allow_online: bool,
) -> Result<RunOutcome> {
    let parallel = effective_parallel(cli.parallel, config)?;
    // 只有 install 判定“已装/中断/该升级”，update 的语义就是跟到最新补丁。
    let install_cmd = matches!(&cli.cmd, Cmd::Install { .. });
    let force = matches!(&cli.cmd, Cmd::Install { force: true, .. });
    let backup = !match &cli.cmd {
        Cmd::Install {
            no_backup_php_ext_ini,
            ..
        }
        | Cmd::Update {
            no_backup_php_ext_ini,
            ..
        }
        | Cmd::Add {
            no_backup_php_ext_ini,
            ..
        }
        | Cmd::Remove {
            no_backup_php_ext_ini,
            ..
        } => *no_backup_php_ext_ini,
        _ => false,
    };

    let mut scheme_list: Option<String> = None;
    let (target_spec, components, target_args, download, goal, json) = match &cli.cmd {
        Cmd::List { target, .. } => (
            target.clone().unwrap_or_default(),
            ComponentArgs::default(),
            TargetArgs::default(),
            None,
            None,
            false,
        ),
        Cmd::Graph { target } => (
            target.clone(),
            ComponentArgs::default(),
            TargetArgs::default(),
            None,
            None,
            false,
        ),
        Cmd::Scheme { target, .. } => (
            target.clone(),
            ComponentArgs::default(),
            TargetArgs::default(),
            None,
            None,
            false,
        ),
        Cmd::Plan {
            target,
            goal,
            json,
            components,
            targets,
            ..
        } => (
            target.clone(),
            components.clone(),
            targets.clone(),
            None,
            goal.clone(),
            *json,
        ),
        Cmd::Install {
            target,
            components,
            targets,
            download,
            ..
        } => {
            let (one, list) = install_target(target)?;
            if list.is_some()
                && (!components.component.is_empty()
                    || !components.disable_default_component.is_empty())
            {
                bail!("方案列表不能和 --component / --disable-default-component 一起用");
            }
            scheme_list = list;
            (
                one,
                components.clone(),
                targets.clone(),
                Some(download.clone()),
                None,
                false,
            )
        }
        Cmd::Update {
            target,
            components,
            targets,
            download,
            ..
        } => (
            single_target(target, "install")?,
            components.clone(),
            targets.clone(),
            Some(download.clone()),
            None,
            false,
        ),
        Cmd::Add {
            target,
            component,
            dep,
            download,
            ..
        } => (
            target.clone(),
            ComponentArgs {
                component: vec![component.clone()],
                dep: dep.clone(),
                ..ComponentArgs::default()
            },
            TargetArgs::default(),
            Some(download.clone()),
            None,
            false,
        ),
        Cmd::Remove {
            target, download, ..
        } => (
            target.clone(),
            ComponentArgs::default(),
            TargetArgs::default(),
            Some(download.clone()),
            None,
            false,
        ),
        Cmd::Audit { target, json } => (
            target.clone(),
            ComponentArgs::default(),
            TargetArgs::default(),
            None,
            None,
            *json,
        ),
        Cmd::Catalog { .. } => (
            String::new(),
            ComponentArgs::default(),
            TargetArgs::default(),
            None,
            None,
            false,
        ),
    };
    let cli_targets = if target_args.targets.is_empty() {
        None
    } else {
        Some(targets::parse_cli(&target_args.targets)?)
    };
    let (product_name, requested) = split_target(&target_spec)?;

    let catalog = Catalog::load_layered(
        &recipes.join("catalog.toml"),
        &layout::catalog_dir(&cli.root).join("components"),
    )
    .with_context(|| format!("加载 {}", recipes.display()))?;
    // 先按 catalog 把 `php@8.5` / `php@8.5.12` 归一成系列，recipe 和 bundle 都按系列找。
    let series = catalog.find_product_series(&product_name, &requested)?;
    let explicit_patch = requested != series;
    let metas = recipe::load_meta(recipes, &catalog)?;
    let product_meta = metas
        .iter()
        .find(|meta| {
            meta.kind == Kind::Product
                && meta.name == product_name
                && meta.series.as_deref() == Some(series.as_str())
        })
        .ok_or_else(|| {
            anyhow!(
                "没有 {product_name}@{series} 的 recipe（可用：{}）",
                available_products(&metas, &product_name)
            )
        })?;
    let state_file = state::state_path(&cli.root, &product_name, &series);
    let mut project = ProjectState::load(&state_file)?;
    project.check(&product_name, &series)?;

    let update_cmd = matches!(&cli.cmd, Cmd::Update { .. });
    let lock = lock_requested(&cli.cmd);
    if lock && scheme_list.is_some() {
        bail!(
            "--lock 不能和完整方案列表一起用；两个来源同时决定版本会互相覆盖。\n  \
             换机器复现请用 phprt scheme {product_name}@{series} --lock"
        );
    }
    if lock && project.installed_version.is_empty() {
        bail!(
            "当前没有安装完成的产品，--lock 没有可锁定的版本；\n  \
             先 phprt install {product_name}@{series}，\
             或用 phprt scheme {product_name}@{series} --lock 导出源项目的精确方案"
        );
    }
    // 默认沿用 state 记录的产品补丁；只有 update 或显式写补丁才跟 catalog 前进。
    let version = select_product_patch(
        &catalog,
        &product_name,
        &series,
        &requested,
        explicit_patch,
        &project.installed_version,
        update_cmd,
    )?;

    let bundle_path = recipes
        .join("bundles")
        .join(&product_name)
        .join(format!("{series}.toml"));
    let bundle = Bundle::load(&bundle_path)?;
    bundle.check_product(&product_name)?;

    let bundled = product_meta.bundled.clone();

    let default_names: Vec<String> = bundle.default_components.keys().cloned().collect();
    for name in &default_names {
        if !bundled.iter().any(|item| item == name) {
            bail!(
                "bundle {product_name}/{series}.toml 的 default_components 里 {name} \
                 不是该产品的 bundled 组件"
            );
        }
        if !metas
            .iter()
            .any(|meta| meta.kind == Kind::Component && &meta.name == name)
        {
            bail!(
                "bundle {product_name}/{series}.toml 的 default_components 里 {name} 没有 component recipe"
            );
        }
    }

    let raw = match &scheme_list {
        Some(list) => recipe::preparse_scheme(list)?,
        None => recipe::preparse(&components.component)?,
    };
    let mut disabled_defaults = BTreeSet::new();
    for name in &components.disable_default_component {
        if !disabled_defaults.insert(name.clone()) {
            bail!("--disable-default-component 重复指定了 {name}");
        }
        if !default_names.iter().any(|item| item == name) {
            let available = if default_names.is_empty() {
                "无".to_owned()
            } else {
                default_names.join(", ")
            };
            bail!(
                "--disable-default-component {name} 不是 {product_name}@{series} 的默认组件（默认：{available}）"
            );
        }
        if raw.iter().any(|item| &item.name == name) {
            bail!("--component 和 --disable-default-component 同时指定了 {name}");
        }
    }

    let cli_pins = pins_from_cli(&catalog, &raw, &components.dep, &metas)?;

    // 方案列表是完整方案：直接替换，不补 bundle 默认集。
    // 这里只按名字铺开，mode / feature / dep 在 recipe 加载后由 parse_scheme 校验。
    if scheme_list.is_some() {
        project.components = raw
            .iter()
            .map(|item| (item.name.clone(), state::ComponentState::default()))
            .collect();
        project.active = true;
    }
    // 首次方案从 bundle 的默认组件开始；已有 state 时以 state 为准。
    // scheme 只导出已保存的方案，不凭空展示“默认会装什么”。
    let seeded_defaults = !project.active && !matches!(&cli.cmd, Cmd::Scheme { .. });
    if seeded_defaults {
        for name in &default_names {
            if disabled_defaults.contains(name) {
                continue;
            }
            project.components.insert(
                name.clone(),
                state::ComponentState {
                    mode: String::new(),
                    installed: false,
                    ..state::ComponentState::default()
                },
            );
        }
        project.active = true;
    } else {
        for name in &disabled_defaults {
            project.remove_component(name);
        }
    }

    let mut selected: Vec<String> = project.components.keys().cloned().collect();
    for item in &raw {
        if !selected.contains(&item.name) {
            selected.push(item.name.clone());
        }
    }

    // add / remove 在解析绑定之前调整方案，避免给已移除的组件做校验。
    match &cli.cmd {
        Cmd::Add { component, .. } => {
            let name = raw
                .first()
                .map(|item| item.name.clone())
                .ok_or_else(|| anyhow!("缺少组件名"))?;
            if !metas
                .iter()
                .any(|meta| meta.kind == Kind::Component && meta.name == name)
            {
                bail!("未知组件 component:{name}");
            }
            if !selected.contains(&name) {
                selected.push(name.clone());
            }
            let _ = component;
        }
        Cmd::Remove { name, .. } => {
            if !project.remove_component(name) {
                bail!("component:{name} 不在构建方案里");
            }
            selected.retain(|item| item != name);
        }
        _ => {}
    }

    // 第一遍只按用户显式线和 state.bindings 取线，补丁仍取 catalog 当前。
    // bindings 里已删除组件的残留不加进来：它不是当前方案的节点。
    // catalog 里已经没有的 line 也不在这里报错：它可能只是已删除组件留下的
    // stale binding。真正的 pin 在第二遍按当前闭包生成，闭包内的失效线仍会报错。
    let mut prelim_lines = project.bindings.clone();
    prelim_lines.retain(|name, line| {
        (selected.contains(name) || metas.iter().any(|m| m.kind == Kind::Dep && &m.name == name))
            && catalog
                .lines(name)
                .is_some_and(|lines| lines.contains_key(line))
    });
    for (name, pin) in &cli_pins {
        prelim_lines.insert(name.clone(), pin.line().to_owned());
    }
    let mut bindings = bind::resolve(
        &catalog,
        &bundle,
        &metas,
        &product_name,
        &series,
        &selected,
        &lock::EffectivePins {
            lines: prelim_lines,
            versions: BTreeMap::new(),
        },
    )?;

    let mut ctx = bind::context(&product_name, &series, &version, &bindings);
    let mut reg = recipe::load_targets(
        recipes,
        &catalog,
        &product_name,
        &series,
        &version,
        &metas,
        &bindings.lines,
        &ctx,
    )?;

    let mut preset_sel = bundle::presets(&reg, &bundle, &series)?;
    if seeded_defaults {
        let default_sel = bundle::selection(&reg, &bundle, &series)?;
        for (name, spec) in &default_sel.per_component {
            let Some(entry) = project.components.get_mut(name) else {
                continue;
            };
            if entry.mode.is_empty()
                && let Some(mode) = &spec.mode
            {
                entry.mode = mode.clone();
            }
            entry.deps = spec.deps.clone();
            entry.features = spec
                .features
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect();
        }
    }

    project.validate(&reg)?;
    let selected_targets = targets::effective(
        &reg,
        &bundle.targets_default,
        &project.targets,
        cli_targets.as_ref(),
    )?;
    targets::apply(&mut reg, &selected_targets)?;
    project.targets = selected_targets.clone();

    let mut cli_sel = match &scheme_list {
        Some(list) => recipe::parse_scheme(&reg, list)?,
        None => Selection::parse(&reg, &components.component)?,
    };
    let mut product_id = reg.product_any()?.id();

    // 第二遍：算出当前方案真正会用到的节点，再和 state 求交生成 EffectivePins。
    // 不能把整份 state 铺成 pin，否则已经被 remove 的组件和历史依赖会重新参与解析。
    if uses_resolver(&cli.cmd) {
        let names = scheme_names(&project, &raw, None);
        project.apply_scheme(&mut reg, &names)?;
        let sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
        let features = reg_features(&reg, &sel, &project);
        let order = plan::resolve(&reg, &product_id, &sel, &features)?;
        let desired: BTreeSet<String> = order.iter().map(|target| target.name.clone()).collect();
        let bundled_set: BTreeSet<String> = bundled.iter().cloned().collect();
        let pins = lock::effective_pins(&project, &desired, &bundled_set, &cli_pins, lock);
        let changed = desired.iter().any(|name| {
            pins.lines
                .get(name)
                .is_some_and(|line| bindings.lines.get(name) != Some(line))
                || pins.versions.get(name).is_some_and(|version| {
                    bindings.artifacts.get(name).map(|a| &a.version) != Some(version)
                })
        });
        if changed {
            // 第二遍换线可能改变 recipe 求值结果，从而改变依赖闭包。这里只记录
            // 第一遍的求解输入，第二遍结束后再算一次做比较；不再重新生成 pins。
            let inputs_before = plan::inputs(&reg, &sel, &features, &order)?;
            bindings = bind::resolve(
                &catalog,
                &bundle,
                &metas,
                &product_name,
                &series,
                &selected,
                &pins,
            )?;
            ctx = bind::context(&product_name, &series, &version, &bindings);
            reg = recipe::load_targets(
                recipes,
                &catalog,
                &product_name,
                &series,
                &version,
                &metas,
                &bindings.lines,
                &ctx,
            )?;
            preset_sel = bundle::presets(&reg, &bundle, &series)?;
            cli_sel = match &scheme_list {
                Some(list) => recipe::parse_scheme(&reg, list)?,
                None => Selection::parse(&reg, &components.component)?,
            };
            product_id = reg.product_any()?.id();
            project.validate(&reg)?;
            targets::apply(&mut reg, &selected_targets)?;
            // 重新加载的 reg 不带方案里的组件列表，检查前先按方案挂回去。
            project.apply_scheme(&mut reg, &names)?;
            let sel_after = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
            let features_after = reg_features(&reg, &sel_after, &project);
            let order_after = plan::resolve(&reg, &product_id, &sel_after, &features_after)?;
            let inputs_after = plan::inputs(&reg, &sel_after, &features_after, &order_after)?;
            check_closure_change(&inputs_before, &inputs_after, lock)?;
        }
    }
    // 在线检查放在真正下载/编译之前；取到新数据后整条命令重跑一遍，
    // 让版本选择、绑定和方案都按新 catalog 重新算。
    if allow_online
        && let Some(download) = download.as_ref()
        && let Some(kind) = remote_kind(&cli.cmd)
    {
        if download.refresh && lock {
            bail!("--refresh 不能和 --lock 一起用");
        }
        let names = scheme_names(
            &project,
            &raw,
            match &cli.cmd {
                Cmd::Remove { name, .. } => Some(name.as_str()),
                _ => None,
            },
        );
        project.apply_scheme(&mut reg, &names)?;
        let sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
        let features = reg_features(&reg, &sel, &project);
        let order = plan::resolve(&reg, &product_id, &sel, &features)?;
        let will_build = match &cli.cmd {
            Cmd::Install { .. } => {
                install_would_build(
                    &project,
                    &product_name,
                    &version,
                    &order,
                    &sel,
                    &components,
                    &target_args,
                    &scheme_list,
                    download.downloads,
                    force,
                ) || !matches!(
                    marker::site(&cli.root, &product_name, &project.installed_version),
                    marker::Site::Complete(_)
                )
            }
            Cmd::Update { .. } => true,
            Cmd::Add {
                component, force, ..
            } => {
                let name = component.split(['=', ',']).next().unwrap_or("");
                !matches!(
                    marker::site(&cli.root, &product_name, &project.installed_version),
                    marker::Site::Absent
                ) && (*force
                    || !project
                        .components
                        .get(name)
                        .is_some_and(|state| state.installed))
            }
            Cmd::Remove { name, .. } => remove_needs_rebuild(
                &reg,
                name,
                &cli.root,
                &product_name,
                &project.installed_version,
            )?,
            _ => false,
        };
        let items = remote_items(
            &order,
            &catalog,
            &bindings,
            &project,
            &product_name,
            &series,
        );
        let request = catalog_remote::Request {
            kind,
            product: &product_name,
            series: &series,
            add_name: match &cli.cmd {
                Cmd::Add { component, .. } => component.split(['=', ',']).next(),
                _ => None,
            },
            items,
            will_build,
            enabled: config.catalog.ask_before_online_update
                && !download.no_refresh
                && !download.offline
                && !lock,
            force: download.refresh,
            default_yes: if kind == catalog_remote::Kind::Remove {
                false
            } else {
                config.catalog_default_answer()?
            },
            mode: download.ui,
            jobs: download.download_jobs,
            host_jobs: download.download_host_jobs,
            proxy: download.proxy.clone(),
            github_proxy: config.github_proxy()?,
            root: &cli.root,
            catalog: &catalog,
        };
        if catalog_remote::refresh(&request)?.changed {
            return Ok(RunOutcome::Retry);
        }
    }
    // install 的绑定提示推迟到现场判定之后打印，拒绝时不先刷“注意”；
    // scheme 的输出要能直接复制执行，也不要提示。
    if !install_cmd && !matches!(&cli.cmd, Cmd::Scheme { .. }) {
        for note in &bindings.notes {
            eprintln!("注意: {note}");
        }
    }

    match cli.cmd.clone() {
        Cmd::List {
            installed, pending, ..
        } => {
            println!("series {series}（{product_name} {version}）");
            print!(
                "{}",
                render::component_list_text(&reg, &project, installed, pending)
            );
        }
        Cmd::Graph { .. } => {
            let names = scheme_names(&project, &raw, None);
            project.apply_scheme(&mut reg, &names)?;
            let sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
            let features = reg_features(&reg, &sel, &project);
            let order = plan::resolve(&reg, &product_id, &sel, &features)?;
            print!("{}", render::graph_text(&reg, &order, &sel, &features)?);
        }
        Cmd::Plan { .. } => {
            let names = scheme_names(&project, &raw, None);
            project.apply_scheme(&mut reg, &names)?;
            let sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
            let features = reg_features(&reg, &sel, &project);
            let root = plan_root(
                &reg,
                &product_id,
                &product_name,
                &series,
                &version,
                goal.as_deref(),
            )?;
            let order = plan::resolve(&reg, &root, &sel, &features)?;
            let prepared = prepare_order(
                &reg,
                &sel,
                order,
                &cli.root,
                &project,
                lock_requested(&cli.cmd),
                true,
                false,
            )?;
            let sim = render::build(
                &reg,
                &prepared.order,
                &cli.root.display().to_string(),
                &sel,
                &features,
                &prepared,
            )?;
            if json {
                println!("{}", render::to_json(&sim)?);
            } else {
                print!("{}", render::to_text(&sim));
            }
        }
        Cmd::Install { .. } | Cmd::Update { .. } => {
            let names = scheme_names(&project, &raw, None);
            project.apply_scheme(&mut reg, &names)?;
            let sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
            let features = reg_features(&reg, &sel, &project);

            let download = download.expect("install/update 一定有下载参数");
            let mut scheme_bindings = project.bindings.clone();
            scheme_bindings.extend(pin_lines(&cli_pins));
            let scheme = marker::Scheme::new(
                &product_name,
                &series,
                &sel,
                &scheme_bindings,
                &selected_targets,
                &is_dep_kind(&metas),
            );

            // 现场判定放在下载之前：拒绝重复安装时不碰网络、不写文件。
            // `--downloads` 只取源码，不算安装，所以不判定。
            let tree_display = layout::display_path(
                &cli.root,
                &layout::tree_path(&cli.root, &product_name, &version),
            );
            let installed_tree = if project.installed_version.is_empty() {
                tree_display.clone()
            } else {
                layout::display_path(
                    &cli.root,
                    &layout::tree_path(&cli.root, &product_name, &project.installed_version),
                )
            };
            let command = command_prefix(cli);
            let order = plan::resolve(&reg, &product_id, &sel, &features)?;
            let resolution = marker::resolution_digest(&version, &order);
            let inputs = marker::inputs_digest(&order, &sel);
            let prepared = prepare_order(
                &reg,
                &sel,
                order,
                &cli.root,
                &project,
                lock_requested(&cli.cmd),
                !download.downloads,
                !download.downloads,
            )?;

            let target_args = targets::scheme_args(&selected_targets);
            let mut plan = marker::Plan::Install;
            if install_cmd && !download.downloads {
                plan = marker::decide(
                    &marker::InstallFacts {
                        product: &product_name,
                        series: &series,
                        target: &version,
                        installed: &project.installed_version,
                        explicit_patch,
                        site: marker::site(&cli.root, &product_name, &version),
                        scheme: &scheme.digest,
                        resolution: &resolution,
                        inputs: &inputs,
                        requested_command: &scheme.command,
                        tree: tree_display.clone(),
                        installed_tree,
                        command: &command,
                        target_args: &target_args,
                    },
                    force,
                )?;
                if plan == marker::Plan::Continue {
                    println!(
                        "上次安装没有收尾（缺 {tree_display}/{}），继续安装",
                        marker::FILE_NAME
                    );
                }
                if plan == marker::Plan::Rebuild {
                    println!("已装版本不变，构建参数有变化：按签名增量重编，不备份原安装树");
                }
            }
            // install 的绑定提示放在判定之后：拒绝时不该先刷“注意”。
            if install_cmd {
                for note in &bindings.notes {
                    eprintln!("注意: {note}");
                }
            }

            let sim = render::build(
                &reg,
                &prepared.order,
                &cli.root.display().to_string(),
                &sel,
                &features,
                &prepared,
            )?;
            print!("{}", render::to_text(&sim));
            if fetch_sources(&cli.root, &reg, &prepared.order, &download, config)? {
                return Ok(RunOutcome::Done);
            }

            // 下载、解压都成功了才动现场：--force 重装时把旧树改名备份，
            // 中途失败不会让原来的安装不可用。
            if plan == marker::Plan::InstallAfterBackup {
                let from = marker::tree_path(&cli.root, &product_name, &version);
                let to = marker::backup_tree(&cli.root, &product_name, &version)?;
                println!(
                    "已把旧版本改名：{} → {}",
                    relative(&cli.root, &from),
                    relative(&cli.root, &to)
                );
            }

            build::run(
                &reg,
                &prepared.order,
                &sel,
                &features,
                &build::Options {
                    root: &cli.root,
                    recipes,
                    ui: download.ui,
                    parallel,
                    scheme: &scheme,
                    tools: &prepared.tools,
                    toolchain: prepared.toolchain.resolved.as_ref(),
                    backup,
                },
            )?;

            project.bindings.extend(pin_lines(&cli_pins));
            project.installed_version = version.clone();
            // install --component 只影响这次构建，不写进 state；
            // 持久化组件方案用 add / remove。
            let persist: Vec<String> = project.components.keys().cloned().collect();
            project.sync_selection(&reg, &sel, &persist)?;
            project.set_deps(&prepared.order, &features);
            project.set_tools(&prepared.tools);
            project.set_installed_all();
            project.save(&state_file)?;
            println!();
            println!("已写入 {}", state_file.display());
        }
        Cmd::Scheme { lock: locked, .. } => {
            if !project.active {
                bail!("当前没有构建过 {product_name} {series}");
            }
            let sel = project.as_selection();
            if locked {
                if project.installed_version.is_empty() {
                    bail!(
                        "当前没有安装完成的产品，scheme --lock 没有可导出的补丁；\
                         先 phprt install {product_name}@{series}"
                    );
                }
                // 只导出当前闭包里的节点：已删除组件留下的历史依赖不写进命令。
                let names = project.scheme_with(&[]);
                project.apply_scheme(&mut reg, &names)?;
                let features = reg_features(&reg, &sel, &project);
                let order = plan::resolve(&reg, &product_id, &sel, &features)?;
                let desired: BTreeSet<String> = order.iter().map(|t| t.name.clone()).collect();
                let bundled_set: BTreeSet<String> = bundled.iter().cloned().collect();
                let is_dep = is_dep_kind(&metas);
                // 版本来源：state 记录优先；state 没记录的闭包节点用本次解析的
                // artifact 补齐（bindings 已经过两遍解析，反映 state 里的线）。
                let exact = lock::exact_versions(
                    &catalog,
                    &project,
                    &product_name,
                    &bundled_set,
                    &is_dep,
                    &desired,
                    &bindings,
                )?;
                // 旧 state 或手工写入的闭包外 binding 不导出：命令里只保留当前节点。
                let scheme_lines: BTreeMap<String, String> = project
                    .bindings
                    .iter()
                    .filter(|(name, _)| desired.contains(*name))
                    .map(|(name, line)| (name.clone(), line.clone()))
                    .collect();
                println!(
                    "{}",
                    marker::scheme_command_locked(
                        &product_name,
                        &series,
                        &project.installed_version,
                        &sel,
                        &scheme_lines,
                        &selected_targets,
                        &exact,
                        &is_dep
                    )
                );
            } else {
                println!(
                    "{}",
                    marker::scheme_command(
                        &product_name,
                        &series,
                        &sel,
                        &project.bindings,
                        &selected_targets,
                        &is_dep_kind(&metas)
                    )
                );
            }
        }
        Cmd::Audit { .. } => {
            let names = scheme_names(&project, &raw, None);
            project.apply_scheme(&mut reg, &names)?;
            let product = reg.product_any()?.clone();
            let report = audit::run(
                &reg,
                &audit::Options {
                    root: &cli.root,
                    product: &product,
                    series: &series,
                    bundle: &bundle,
                    tools: &project.tools,
                },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", audit::text(&report));
            }
            let missing = report.missing();
            let unaudited: Vec<String> = report
                .components
                .iter()
                .filter(|component| component.audited && component.options.is_empty())
                .map(|component| format!("{}（{}）", component.id, component.note))
                .collect();
            let stale: Vec<_> = report
                .components
                .iter()
                .filter(|component| component.audited)
                .flat_map(|component| {
                    component
                        .stale
                        .iter()
                        .map(move |(flag, _)| (component.id.clone(), flag.clone()))
                })
                .collect();
            if !missing.is_empty() || !stale.is_empty() || !unaudited.is_empty() {
                let mut lines = Vec::new();
                for (component, row) in missing {
                    lines.push(format!("{} {}", component.id, row.option));
                }
                for (id, flag) in stale {
                    lines.push(format!("{id} {flag}（ignored 但上游没有这个选项）"));
                }
                lines.extend(unaudited);
                bail!(
                    "{} 项没有通过 audit：\n  {}\n\
                     处置方式：写进 features，或用 ignored 写明不暴露的理由；\
                     源码没解压的先安装一次",
                    lines.len(),
                    lines.join("\n  ")
                );
            }
        }
        // catalog update 在前面已经 early-return，这里是编译要求的兜底。
        Cmd::Catalog { .. } => unreachable!("catalog update 已经在前面处理"),
        Cmd::Add { force, .. } => {
            let download = download.expect("add 一定带下载参数");
            let name = raw
                .first()
                .map(|item| item.name.clone())
                .ok_or_else(|| anyhow!("缺少组件名"))?;
            let product = reg.product_any()?.clone();
            let component = reg.component(&name)?.clone();
            let explicit = cli_sel
                .per_component
                .get(&name)
                .and_then(|spec| spec.mode.as_deref());
            let mode = manage::mode_for_add(&reg, &product, &series, &component, explicit)?;

            let tree_version = project.installed_version.clone();
            let tree = marker::tree_path(&cli.root, &product_name, &tree_version);
            let site = if tree_version.is_empty() {
                marker::Site::Absent
            } else {
                marker::site(&cli.root, &product_name, &tree_version)
            };
            let installed = match &site {
                marker::Site::Complete(installed) => Some(installed.clone()),
                _ => None,
            };

            let in_scheme = project.components.contains_key(&name);
            let stored_mode = project
                .components
                .get(&name)
                .map(|entry| entry.mode.clone())
                .unwrap_or_default();
            if in_scheme && !stored_mode.is_empty() && stored_mode != mode {
                bail!(
                    "{name} 已经在方案里，挂载方式是 {stored_mode}；\
                     要换成 {mode} 先 phprt remove {product_name}@{series} {name}"
                );
            }

            manage::check_add(
                &product,
                &component,
                &mode,
                &tree,
                installed.as_ref(),
                force,
            )?;

            // 已在方案里、产物齐全：不重复加，也不重编（--force 才重编）。
            if in_scheme && !force {
                println!("{name} 已在方案里（{mode}），产物齐全");
                return Ok(RunOutcome::Done);
            }

            // 静态组件要把产品整个重编，和 remove 一样先提示代价、确认。
            let rebuild = reg.product_mode(&component, &mode)?.strategy_kind()? == Strategy::Fold;
            if rebuild && !matches!(site, marker::Site::Absent) && !force {
                let tree_display = layout::display_path(
                    &cli.root,
                    &layout::tree_path(&cli.root, &product_name, &tree_version),
                );
                let warning = manage::rebuild_warning(&product, &name, &mode, &tree_display);
                if !manage::interactive() {
                    bail!("{warning}\n\n非交互环境用 --force 确认重编");
                }
                println!("{warning}");
                if !manage::confirm("继续？")? {
                    bail!("已取消；确认重编就再跑一次并回答 y，或用 --force");
                }
            }
            if !in_scheme {
                let mut spec = preset_sel
                    .per_component
                    .get(&name)
                    .cloned()
                    .unwrap_or_default();
                spec.mode = Some(mode.clone());
                project.add_component_with_preset(&reg, &name, Some(&spec))?;
            }

            let names = scheme_names(&project, &raw, None);
            project.apply_scheme(&mut reg, &names)?;
            let sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
            let features = reg_features(&reg, &sel, &project);
            let mut scheme_bindings = project.bindings.clone();
            scheme_bindings.extend(pin_lines(&cli_pins));
            let scheme = marker::Scheme::new(
                &product_name,
                &series,
                &sel,
                &scheme_bindings,
                &selected_targets,
                &is_dep_kind(&metas),
            );
            let order = plan::resolve(&reg, &product_id, &sel, &features)?;

            // 没有安装树时不代跑 install，只记方案。
            if matches!(site, marker::Site::Absent) {
                project.bindings.extend(pin_lines(&cli_pins));
                project.sync_selection(&reg, &sel, &names)?;
                project.set_deps(&order, &features);
                project.save(&state_file)?;
                println!();
                println!(
                    "已加入 {name}（{mode}）；这个版本还没装过，先 phprt install {product_name}@{series}"
                );
                println!("已写入 {}", state_file.display());
                return Ok(RunOutcome::Done);
            }

            let prepared = prepare_order(
                &reg,
                &sel,
                order,
                &cli.root,
                &project,
                lock_requested(&cli.cmd),
                !download.downloads,
                !download.downloads,
            )?;
            if fetch_sources(&cli.root, &reg, &prepared.order, &download, config)? {
                return Ok(RunOutcome::Done);
            }
            build::run(
                &reg,
                &prepared.order,
                &sel,
                &features,
                &build::Options {
                    root: &cli.root,
                    recipes,
                    ui: download.ui,
                    parallel,
                    scheme: &scheme,
                    tools: &prepared.tools,
                    toolchain: prepared.toolchain.resolved.as_ref(),
                    backup,
                },
            )?;

            project.bindings.extend(pin_lines(&cli_pins));
            project.installed_version = version.clone();
            project.sync_selection(&reg, &sel, &names)?;
            project.set_deps(&prepared.order, &features);
            project.set_tools(&prepared.tools);
            project.set_installed_all();
            project.save(&state_file)?;
            println!();
            println!("已加入 {name}（{mode}）并完成编译");
            println!("已写入 {}", state_file.display());
        }
        Cmd::Remove {
            name, force, lock, ..
        } => {
            let download = download.expect("remove 一定带下载参数");
            let product = reg.product_any()?.clone();
            let component = reg.component(&name)?.clone();

            // 方案里已经从 project / selected 移除了。
            let names = scheme_names(&project, &raw, Some(&name));
            project.apply_scheme(&mut reg, &names)?;
            let mut sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
            sel.per_component.remove(&name);
            let features = reg_features(&reg, &sel, &project);
            let order = plan::resolve(&reg, &product_id, &sel, &features)?;
            // remove 已经得到最终闭包：闭包外的显式线不再有语义意义。
            // 必须在生成 Scheme 之前收窄，marker 里的指纹/命令才和 state 一致。
            project.bindings.extend(pin_lines(&cli_pins));
            let desired: BTreeSet<String> = order.iter().map(|t| t.name.clone()).collect();
            project.prune_bindings(&desired);
            let scheme = marker::Scheme::new(
                &product_name,
                &series,
                &sel,
                &project.bindings,
                &selected_targets,
                &is_dep_kind(&metas),
            );

            let tree_version = project.installed_version.clone();
            let tree = marker::tree_path(&cli.root, &product_name, &tree_version);
            let site = if tree_version.is_empty() {
                marker::Site::Absent
            } else {
                marker::site(&cli.root, &product_name, &tree_version)
            };

            let mut rebuilt = false;
            let mut stale_artifacts: Vec<String> = Vec::new();
            match &site {
                marker::Site::Complete(installed) => {
                    let removal = manage::remove_plan(&reg, &component, installed, &tree, force)?;
                    if removal.fold {
                        stale_artifacts = removal.facts.artifacts.clone();
                        let tree_display = layout::display_path(
                            &cli.root,
                            &layout::tree_path(&cli.root, &product_name, &tree_version),
                        );
                        let warning = manage::rebuild_warning(
                            &product,
                            &name,
                            &removal.facts.mode,
                            &tree_display,
                        );
                        if !force {
                            if !manage::interactive() {
                                bail!("{warning}\n\n非交互环境用 --force 确认重编");
                            }
                            println!("{warning}");
                            if !manage::confirm("继续？")? {
                                bail!("已取消；确认重编就再跑一次并回答 y，或用 --force");
                            }
                        }
                        rebuilt = true;
                    } else {
                        let deleted = manage::apply(
                            &cli.root,
                            &tree,
                            &removal,
                            product.profile.backup.as_ref(),
                            backup,
                        )?;
                        if deleted.is_empty() {
                            println!(
                                "已移除 {name}（{}），磁盘上没有可删的文件",
                                removal.facts.mode
                            );
                        } else {
                            println!(
                                "已移除 {name}（{}），删掉 {} 个文件：",
                                removal.facts.mode,
                                deleted.len()
                            );
                            for item in &deleted {
                                println!("  {item}");
                            }
                        }
                    }
                }
                _ => println!("这个版本没有装完，只更新方案"),
            }

            if rebuilt {
                let prepared = prepare_order(
                    &reg,
                    &sel,
                    order.clone(),
                    &cli.root,
                    &project,
                    lock,
                    !download.downloads,
                    !download.downloads,
                )?;
                if !fetch_sources(&cli.root, &reg, &prepared.order, &download, config)? {
                    build::run(
                        &reg,
                        &prepared.order,
                        &sel,
                        &features,
                        &build::Options {
                            root: &cli.root,
                            recipes,
                            ui: download.ui,
                            parallel,
                            scheme: &scheme,
                            tools: &prepared.tools,
                            toolchain: prepared.toolchain.resolved.as_ref(),
                            backup,
                        },
                    )?;
                    project.set_tools(&prepared.tools);
                    let deleted = manage::delete_stale_artifacts(&tree, &stale_artifacts)?;
                    if !deleted.is_empty() {
                        println!("已删除旧产物 {} 个：", deleted.len());
                        for item in &deleted {
                            println!("  {item}");
                        }
                    }
                }
            }

            if rebuilt {
                project.installed_version = version.clone();
                project.set_installed_all();
            }
            project.sync_selection(&reg, &sel, &names)?;
            if rebuilt {
                project.set_deps(&order, &features);
            } else {
                // 不重编时不要按当前 catalog 重写版本：记录的是上次真正编过的补丁。
                project.prune_deps(&order, &features);
            }
            project.save(&state_file)?;

            // 动态组件删完不用重编，标记里的组件和方案指纹跟着改。
            if let marker::Site::Complete(installed) = &site
                && !rebuilt
            {
                let mut record = installed.clone();
                record.components.retain(|item| item.name != name);
                record.scheme = scheme.digest.clone();
                record.resolution = marker::resolution_digest(&version, &order);
                record.inputs = marker::inputs_digest(&order, &sel);
                record.scheme_cmd = scheme.command.clone();
                marker::write(&cli.root, &product_name, &tree_version, &record)?;
            }
            println!("已移除组件 {name}");
            println!("已写入 {}", state_file.display());
        }
    }
    Ok(RunOutcome::Done)
}

/// 一次解析的产物：构建顺序、工具、编译器检查、宿主命令提示。
pub struct Prepared<'a> {
    pub order: Vec<&'a Target>,
    pub tools: tools::Resolution,
    pub toolchain: toolchain::Checked,
    pub host: Option<host_commands::Report>,
}

/// 解析整条构建顺序的工具，并把需要构建的工具节点排到最前面。
///
/// 工具节点只依赖其它工具，所以统一前置是拓扑序；宿主满足的工具不产生节点。
/// 解析始终收集全部缺失项：`strict` 时一次报全，`plan` 只把结果放进 `Sim`。
#[allow(clippy::too_many_arguments)]
fn prepare_order<'a>(
    reg: &'a Registry,
    sel: &Selection,
    order: Vec<&'a Target>,
    root: &Path,
    project: &ProjectState,
    lock: bool,
    check_host: bool,
    strict: bool,
) -> Result<Prepared<'a>> {
    let product = reg.product_any()?;
    let locked = if lock {
        project.locked_tools()
    } else {
        BTreeMap::new()
    };
    let resolution = tools::resolve(
        &order,
        &tools::Options {
            reg,
            product,
            sel,
            build_root: root,
            locked,
            lock,
            lenient: true,
        },
    )?;
    let mut full: Vec<&Target> = Vec::new();
    for id in &resolution.tool_ids {
        let node = reg
            .get(id)
            .ok_or_else(|| anyhow!("工具节点 {id} 不在 registry 里"))?;
        full.push(node);
    }
    full.extend(order);
    let toolchain = if check_host {
        toolchain::check(product)?
    } else {
        toolchain::Checked::default()
    };

    // 缺命令用命令名；feature_check 的 need_packages 用它给的键，和缺命令同一条聚合。
    let mut missing: Vec<String> = Vec::new();
    let mut reasons: BTreeMap<String, String> = BTreeMap::new();
    for item in resolution
        .unresolved
        .iter()
        .filter(|item| item.kind.is_missing_command())
    {
        if item.packages.is_empty() {
            missing.push(item.name.clone());
        } else {
            for name in &item.packages {
                if !item.note.is_empty() {
                    reasons.entry(name.clone()).or_insert(item.note.clone());
                }
            }
            missing.extend(item.packages.iter().cloned());
        }
    }
    let mut seen = BTreeSet::new();
    let blocked: Vec<String> = resolution
        .unresolved
        .iter()
        .filter(|item| !item.kind.is_missing_command())
        .map(|item| item.reason.clone())
        .filter(|reason| seen.insert(reason.clone()))
        .collect();
    let host = if check_host {
        reg.host.report(
            &toolchain.missing,
            &missing,
            reasons.clone(),
            blocked.clone(),
        )
    } else {
        None
    };

    if strict && check_host {
        if let Some(report) = &host {
            bail!("{}", report.error_message());
        }
        if let Some(reason) = blocked.first() {
            bail!("{reason}");
        }
    }

    Ok(Prepared {
        order: full,
        tools: resolution,
        toolchain,
        host,
    })
}

/// 下载并解压；返回 true 表示这次只下载（`--downloads`），不继续编译。
fn fetch_sources(
    root: &Path,
    _reg: &Registry,
    order: &[&Target],
    download: &DownloadArgs,
    config: &config::Config,
) -> Result<bool> {
    let cache = layout::downloads_dir(root);
    let sources = sources::collect(order, root, &cache)?;
    let options = phprt_download::Options {
        jobs: download.download_jobs,
        host_jobs: download.download_host_jobs,
        connections: download.connections,
        timeout: download.timeout,
        proxy: download.proxy.clone(),
        github_proxy: config.github_proxy()?,
        offline: download.offline,
        ui: download.ui,
    };
    phprt_download::run(&sources, &cache, &options)?;

    if download.downloads {
        println!();
        println!("源码已下载到 {}（未解压）", cache.display());
        return Ok(true);
    }

    phprt_extract::run(&sources, download.ui)?;
    Ok(false)
}

/// 判断一个绑定名是不是依赖库：是就走 `--dep`，不是就当组件版本线。
fn is_dep_kind(metas: &[RecipeMeta]) -> impl Fn(&str) -> bool + '_ {
    |name: &str| {
        metas
            .iter()
            .any(|meta| meta.kind == Kind::Dep && meta.name == name)
    }
}

/// `install` 的位置参数：一个产品，后面可以再跟一份完整方案列表。
fn install_target(target: &[String]) -> Result<(String, Option<String>)> {
    match target {
        [one] => Ok((one.clone(), None)),
        [one, list] => {
            // `install php@8.5 nginx@1.29` 仍然是“多个产品”而不是方案列表。
            if list.contains('@') && !list.contains('=') && !list.contains(':') {
                bail!(
                    "install 只接受一个产品，例如 phprt install php@8.5；\
                     多个产品请分多条命令"
                );
            }
            Ok((one.clone(), Some(list.clone())))
        }
        [] => bail!("install 需要指定一个产品，例如 phprt install php@8.5"),
        _ => bail!("install 最多接一个产品和一个方案列表，例如 phprt install php@8.5 'swoole'"),
    }
}

/// recipe 目录的优先级：`--recipes` > phprt.toml 的 `recipes` > `./recipes`。
/// 配置里的相对路径按项目根解析；命令行和默认值按当前目录解析。
/// 允许 `--recipes ""`：空值按“没写”处理。
fn path_value(value: &str) -> std::result::Result<PathBuf, String> {
    Ok(PathBuf::from(value))
}

/// `--internal-dir` / `--build-dir`：命令行优先于 phprt.toml，空值按未指定处理。
fn effective_dir(
    given: Option<&Path>,
    configured: &str,
    default: &str,
    key: &str,
) -> Result<String> {
    let raw = match given {
        Some(path) if !path.as_os_str().is_empty() => path.to_string_lossy().into_owned(),
        _ => configured.to_owned(),
    };
    Ok(config::dir_value(key, &raw)?.unwrap_or_else(|| default.to_owned()))
}

/// 全局模式的目录：命令行 > phprt.toml > 默认；相对路径按 `$HOME` 解析。
fn effective_global_dir(
    given: Option<&Path>,
    configured: &str,
    default: &str,
    key: &str,
) -> Result<String> {
    let raw = match given {
        Some(path) if !path.as_os_str().is_empty() => path.to_string_lossy().into_owned(),
        _ => configured.to_owned(),
    };
    let value = config::dir_value(key, &raw)?.unwrap_or_else(|| default.to_owned());
    Ok(layout::resolve_home(&value)?.display().to_string())
}

/// 下载目录：命令行 > phprt.toml > 模式默认。
/// 本地默认 `<internal>/downloads`（返回 None，让 layout 用默认），全局默认 `~/phprtdownloads`。
/// 相对路径：本地按项目根，全局按 `$HOME`。
fn effective_download_dir(
    given: Option<&Path>,
    configured: Option<&str>,
    root: &Path,
    global: bool,
) -> Result<Option<PathBuf>> {
    let raw = match given {
        Some(path) if !path.as_os_str().is_empty() => path.to_string_lossy().into_owned(),
        _ => configured.unwrap_or("").to_owned(),
    };
    let value = config::dir_value("download_dir", &raw)?;
    match (value, global) {
        (Some(value), true) => Ok(Some(layout::resolve_home(&value)?)),
        (Some(value), false) => Ok(Some(layout::resolve(root, &value))),
        (None, true) => Ok(Some(layout::resolve_home("~/phprtdownloads")?)),
        (None, false) => Ok(None),
    }
}

/// `--backup-dir`：空表示用产品 recipe 的默认位置。
fn effective_dir_opt(
    given: Option<&Path>,
    configured: &Option<String>,
    key: &str,
) -> Result<Option<String>> {
    let raw = match given {
        Some(path) if !path.as_os_str().is_empty() => path.to_string_lossy().into_owned(),
        _ => configured.clone().unwrap_or_default(),
    };
    config::dir_value(key, &raw)
}

/// 配置文件路径：`-c/--config` 优先，否则用 `<root>/phprt.toml`。
fn effective_config(cli: &Cli) -> PathBuf {
    match &cli.config {
        Some(path) if !path.as_os_str().is_empty() => path.clone(),
        _ => cli.root.join(config::FILE_NAME),
    }
}

/// 示例命令的前缀：全局模式带上 `--global --prefix`。
fn command_prefix(cli: &Cli) -> String {
    match &cli.prefix {
        Some(prefix) if cli.global => {
            format!("phprt --global --prefix {}", prefix.display())
        }
        _ => "phprt".to_owned(),
    }
}

/// 编译并发数：命令行 `--parallel` 优先，其次 `phprt.toml` 的 `parallel`，最后 CPU 数 - 1。
fn effective_parallel(given: Option<usize>, config: &config::Config) -> Result<usize> {
    match given {
        Some(0) => bail!("--parallel 至少为 1"),
        Some(n) => Ok(n),
        None => config.parallel(),
    }
}

fn effective_recipes(given: Option<&Path>, root: &Path, config: &config::Config) -> PathBuf {
    if let Some(path) = given
        && !path.as_os_str().is_empty()
    {
        return path.to_path_buf();
    }
    if let Some(path) = config.recipes_path() {
        return if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
    }
    PathBuf::from("recipes")
}

fn single_target(target: &[String], command: &str) -> Result<String> {
    match target {
        [one] => Ok(one.clone()),
        [] => bail!("{command} 需要指定一个产品，例如 phprt {command} php@8.5"),
        _ => bail!(
            "{command} 只接受一个产品，例如 phprt {command} php@8.5；\
             多个产品请分多条命令"
        ),
    }
}

/// 打印路径时优先去掉项目根前缀；全局模式保留绝对路径。
fn relative(root: &Path, path: &Path) -> String {
    layout::display_path(root, path)
}

fn split_target(spec: &str) -> Result<(String, String)> {
    let (product, series) = spec
        .split_once('@')
        .ok_or_else(|| anyhow!("目标要写成 产品@系列，例如 php@8.5；实际是 {spec}"))?;
    if product.is_empty() || series.is_empty() {
        bail!("目标要写成 产品@系列，例如 php@8.5；实际是 {spec}");
    }
    Ok((product.to_owned(), series.to_owned()))
}

fn available_products(metas: &[RecipeMeta], product: &str) -> String {
    let mut out: Vec<String> = metas
        .iter()
        .filter(|meta| meta.kind == Kind::Product && meta.name == product)
        .filter_map(|meta| meta.series.clone())
        .collect();
    out.sort();
    if out.is_empty() {
        "无".to_owned()
    } else {
        out.join(", ")
    }
}

fn print_states(root: &Path) -> Result<()> {
    if let Some(prefix) = layout::global_prefix() {
        let file = prefix.join(".phprt-state.json");
        if !file.is_file() {
            println!(
                "没有已安装的产品（{} 不是 phprt 安装目录）",
                prefix.display()
            );
            return Ok(());
        }
        let state = ProjectState::load(&file)?;
        println!("{:<14} {:<12} tree", "product", "version");
        println!(
            "{:<14} {:<12} {}",
            format!("{}@{}", state.product, state.series),
            state.installed_version,
            prefix.display()
        );
        return Ok(());
    }
    let dir = layout::internal_dir(root);
    if !dir.is_dir() {
        println!("没有已安装的产品（{} 不存在）", dir.display());
        return Ok(());
    }
    let mut rows = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("读不到 {}", dir.display()))? {
        let path = entry?.path();
        if !path.is_dir() || !path.join("state.json").is_file() {
            continue;
        }
        let state = ProjectState::load(&path.join("state.json"))?;
        rows.push((state.product, state.series, state.installed_version, path));
    }
    rows.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    println!("{:<14} {:<12} tree", "product", "version");
    for (product, series, version, path) in rows {
        let root_dir = path.parent().unwrap_or(&path);
        let tree = root_dir
            .join(format!("{product}-{version}"))
            .display()
            .to_string();
        println!(
            "{:<14} {:<12} {}",
            format!("{product}@{series}"),
            version,
            tree
        );
    }
    Ok(())
}

fn pins_from_cli(
    catalog: &Catalog,
    raw: &[RawComponent],
    deps: &[String],
    metas: &[RecipeMeta],
) -> Result<BTreeMap<String, lock::CliPin>> {
    let mut pins = BTreeMap::new();
    for component in raw {
        if let Some(version) = &component.version {
            let Some(meta) = metas
                .iter()
                .find(|meta| meta.kind == Kind::Component && meta.name == component.name)
            else {
                bail!("--component 里指定了未知组件 {} 的版本线", component.name);
            };
            if meta.lines.is_empty() {
                if metas
                    .iter()
                    .any(|meta| meta.kind == Kind::Dep && meta.name == component.name)
                {
                    bail!(
                        "component:{} 是内置组件，没有自己的版本线；\
                         依赖库 {} 的版本请用 --dep {}=<线>",
                        component.name,
                        component.name,
                        component.name
                    );
                }
                bail!("component:{} 是内置组件，没有自己的版本线", component.name);
            }
            pins.insert(
                component.name.clone(),
                lock::cli_pin(catalog, &component.name, version)?,
            );
        }
    }
    for spec in deps {
        let (name, value) = spec
            .find(['=', ':'])
            .map(|i| (&spec[..i], &spec[i + 1..]))
            .ok_or_else(|| anyhow!("--dep {spec} 写法不合法，应为 NAME=LINE"))?;
        let value = value.strip_prefix("version:").unwrap_or(value);
        if name.is_empty() || value.is_empty() {
            bail!("--dep {spec} 写法不合法，应为 NAME=LINE");
        }
        if !metas
            .iter()
            .any(|meta| meta.kind == Kind::Dep && meta.name == name)
        {
            bail!("--dep {name} 不是依赖库");
        }
        if pins
            .insert(name.to_owned(), lock::cli_pin(catalog, name, value)?)
            .is_some()
        {
            bail!("--dep 里重复指定了 {name}");
        }
    }
    Ok(pins)
}

/// 写回 state.bindings 时只存版本线；精确补丁属于一次解析结果，不进 bindings。
fn pin_lines(pins: &BTreeMap<String, lock::CliPin>) -> BTreeMap<String, String> {
    pins.iter()
        .map(|(name, pin)| (name.clone(), pin.line().to_owned()))
        .collect()
}

fn lock_requested(cmd: &Cmd) -> bool {
    match cmd {
        Cmd::Plan { lock, .. }
        | Cmd::Install { lock, .. }
        | Cmd::Add { lock, .. }
        | Cmd::Remove { lock, .. }
        | Cmd::Scheme { lock, .. } => *lock,
        _ => false,
    }
}

/// 需要走 resolver 的命令才生成 EffectivePins；list / scheme / audit 不参与解析。
fn uses_resolver(cmd: &Cmd) -> bool {
    match cmd {
        // 普通 scheme 只导出已保存的方案，不解析；--lock 要先走两遍解析，
        // 才能拿到方案实际选中的 artifact，用来补齐 state 没记录的版本。
        Cmd::Scheme { lock, .. } => *lock,
        _ => matches!(
            cmd,
            Cmd::Graph { .. }
                | Cmd::Plan { .. }
                | Cmd::Install { .. }
                | Cmd::Update { .. }
                | Cmd::Add { .. }
                | Cmd::Remove { .. }
        ),
    }
}

/// 两遍解析之间的闭包校验：第二遍会把 `state` 里的线纳入 pins，可能换线，
/// 而 recipe 契约要求换线不改变 needs / features / tools。
///
/// 这里只比较结果，不重新 bind、不重新生成 pins。`--lock` 必须闭包一致，
/// 否则锁住的不是最终闭包；普通操作只给出警告。
fn check_closure_change(
    before: &BTreeMap<String, plan::NodeInputs>,
    after: &BTreeMap<String, plan::NodeInputs>,
    lock: bool,
) -> Result<()> {
    let added: Vec<&str> = after
        .keys()
        .filter(|id| !before.contains_key(*id))
        .map(String::as_str)
        .collect();
    let removed: Vec<&str> = before
        .keys()
        .filter(|id| !after.contains_key(*id))
        .map(String::as_str)
        .collect();
    if !added.is_empty() || !removed.is_empty() {
        let detail = format!(
            "第二遍换线后依赖闭包变了：新增 [{}]，消失 [{}]",
            added.join(", "),
            removed.join(", ")
        );
        if lock {
            bail!(
                "{detail}\n  \
                 --lock 依赖“换线不改变 needs / features / tools”这条 recipe 契约；\
                 闭包变了就无法保证锁住的是最终闭包。去掉 --lock 重试，或修正 recipe"
            );
        }
        eprintln!(
            "注意: {detail}\n  \
             这违反了“recipe 不能按 version / 线改变 needs / features / tools”的契约；\
             本次按第二遍结果继续，不做第三遍解析。"
        );
        return Ok(());
    }

    let changed: Vec<&str> = before
        .iter()
        .filter(|(id, old)| after.get(*id).is_some_and(|new| new != *old))
        .map(|(id, _)| id.as_str())
        .collect();
    if !changed.is_empty() {
        eprintln!(
            "注意: 第二遍换线后节点集合没变，但这些节点的 plan 输入变了：[{}]\n  \
             当前只校验依赖闭包，不保证 configure、顺序等字段完全等价。",
            changed.join(", ")
        );
    }
    Ok(())
}

/// 产品补丁：显式写补丁 > update 跟 catalog；其他情况沿用 state.installed_version。
fn select_product_patch(
    catalog: &Catalog,
    product: &str,
    series: &str,
    requested: &str,
    explicit_patch: bool,
    installed: &str,
    update: bool,
) -> Result<String> {
    if explicit_patch || update || installed.is_empty() {
        return catalog.product_version_for(product, series, requested);
    }
    catalog
        .product_version_for(product, series, installed)
        .map_err(|_| {
            let versions: Vec<&str> = catalog
                .lines(product)
                .and_then(|lines| lines.get(series))
                .map(|artifacts| {
                    artifacts
                        .iter()
                        .map(|artifact| artifact.version.as_str())
                        .collect()
                })
                .unwrap_or_default();
            anyhow!(
                "state 里的 {product} {installed} 不在 catalog（{series} 线可用：{}）；\
                 用 phprt update {product}@{series} 升级，或在 catalog 里保留旧补丁",
                versions.join(", ")
            )
        })
}

/// online 检查只对这台命令开放。
fn remote_kind(cmd: &Cmd) -> Option<catalog_remote::Kind> {
    match cmd {
        Cmd::Install { .. } => Some(catalog_remote::Kind::Install),
        Cmd::Update { .. } => Some(catalog_remote::Kind::Update),
        Cmd::Add { .. } => Some(catalog_remote::Kind::Add),
        Cmd::Remove { .. } => Some(catalog_remote::Kind::Remove),
        _ => None,
    }
}

/// `install` 这次会不会真的下载或构建；不会时不弹在线检查。
#[allow(clippy::too_many_arguments)]
fn install_would_build(
    project: &ProjectState,
    product: &str,
    version: &str,
    order: &[&Target],
    sel: &Selection,
    components: &ComponentArgs,
    target_args: &TargetArgs,
    scheme_list: &Option<String>,
    downloads: bool,
    force: bool,
) -> bool {
    if downloads
        || force
        || project.installed_version.is_empty()
        || version != project.installed_version
        || scheme_list.is_some()
        || !components.component.is_empty()
        || !components.dep.is_empty()
        || !components.disable_default_component.is_empty()
        || !target_args.targets.is_empty()
    {
        return true;
    }
    for (name, spec) in &sel.per_component {
        let Some(state) = project.components.get(name) else {
            return true;
        };
        if let Some(mode) = &spec.mode
            && mode != &state.mode
        {
            return true;
        }
        if let Some(features) = &spec.features {
            let state_features: BTreeSet<&String> = state.features.iter().collect();
            let selected: BTreeSet<&String> = features.iter().collect();
            if selected != state_features {
                return true;
            }
        }
        if !spec.deps.is_empty() && spec.deps != state.deps {
            return true;
        }
    }
    for &target in order {
        if target.kind == Kind::Tool || target.version.is_empty() {
            continue;
        }
        let state_version = if target.kind == Kind::Product && target.name == product {
            Some(project.installed_version.as_str())
        } else if let Some(state) = project.components.get(&target.name) {
            Some(state.version.as_str())
        } else {
            project
                .deps
                .get(&target.name)
                .map(|state| state.version.as_str())
        };
        match state_version {
            Some(state_version) if state_version == target.version => {}
            _ => return true,
        }
    }
    false
}

fn remove_needs_rebuild(
    reg: &Registry,
    name: &str,
    root: &Path,
    product: &str,
    installed_version: &str,
) -> Result<bool> {
    if installed_version.is_empty() {
        return Ok(false);
    }
    let marker::Site::Complete(installed) = marker::site(root, product, installed_version) else {
        return Ok(false);
    };
    let Some(facts) = installed.component(name) else {
        return Ok(false);
    };
    if facts.mode.is_empty() {
        return Ok(false);
    }
    let component = reg.component(name)?;
    Ok(reg.product_mode(component, &facts.mode)?.strategy_kind()? == Strategy::Fold)
}

/// 当前闭包里能取分片的组件：有 catalog 条目、绑定了线。
fn remote_items(
    order: &[&Target],
    catalog: &Catalog,
    bindings: &bind::Bindings,
    project: &ProjectState,
    product: &str,
    series: &str,
) -> Vec<catalog_remote::Item> {
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    // 产品排第一，后面按构建顺序；进度行和提示清单都更容易读。
    let mut ordered: Vec<&Target> = order.to_vec();
    ordered.sort_by_key(|target| u8::from(target.kind != Kind::Product));
    for target in ordered {
        if target.kind == Kind::Tool {
            continue;
        }
        let Some(lines) = catalog.lines(&target.name) else {
            continue;
        };
        let line = if target.name == product {
            series
        } else {
            bindings
                .lines
                .get(&target.name)
                .map(String::as_str)
                .unwrap_or("")
        };
        if line.is_empty() || !lines.contains_key(line) {
            continue;
        }
        // 同一个组件可能同时以 dep 和 bundled 组件出现（zlib、openssl），只取一次。
        if !seen.insert(target.name.clone()) {
            continue;
        }
        let Some(artifact) = catalog.get(&target.name, line) else {
            continue;
        };
        let installed = if target.name == product {
            project.installed_version.clone()
        } else if let Some(state) = project.components.get(&target.name) {
            if state.installed {
                state.version.clone()
            } else {
                String::new()
            }
        } else if let Some(state) = project.deps.get(&target.name) {
            state.version.clone()
        } else {
            String::new()
        };
        items.push(catalog_remote::Item {
            name: target.name.clone(),
            line: line.to_owned(),
            installed,
            local: artifact.version.clone(),
        });
    }
    items
}

fn scheme_names(
    project: &ProjectState,
    raw: &[RawComponent],
    removed: Option<&str>,
) -> Vec<String> {
    let cli_names: Vec<String> = raw.iter().map(|item| item.name.clone()).collect();
    let mut names = project.scheme_with(&cli_names);
    if let Some(removed) = removed {
        names.retain(|name| name != removed);
    }
    names
}

fn reg_features(
    reg: &Registry,
    sel: &Selection,
    project: &ProjectState,
) -> BTreeMap<String, Vec<String>> {
    let mut features = sel.features(reg);
    if let Ok(product) = reg.product_any() {
        for (dep, deps) in &product.deps {
            let entry = features.entry(dep.clone()).or_default();
            for f in deps {
                if !entry.contains(f) {
                    entry.push(f.clone());
                }
            }
        }
    }
    project.merged_features(features)
}

fn plan_root(
    reg: &Registry,
    product_id: &str,
    product: &str,
    series: &str,
    version: &str,
    goal: Option<&str>,
) -> Result<String> {
    let Some(goal) = goal else {
        return Ok(product_id.to_owned());
    };
    if goal == series
        || goal == version
        || goal == product_id
        || goal == format!("{product}@{version}")
    {
        return Ok(product_id.to_owned());
    }
    if let Some(rest) = goal.strip_prefix("product:") {
        if rest == product_id.trim_start_matches("product:")
            || rest == version
            || rest == format!("{product}@{version}")
            || rest == series
        {
            return Ok(product_id.to_owned());
        }
        bail!("当前加载的是 {product}@{series}（{version}），不能 plan product:{rest}");
    }
    reg.resolve_name(goal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn config_path_prefers_the_flag() {
        let cli = Cli::parse_from(["phprt", "list"]);
        assert_eq!(
            effective_config(&cli),
            PathBuf::from(".").join("phprt.toml")
        );

        let cli = Cli::parse_from(["phprt", "-c", "/tmp/x.toml", "list"]);
        assert_eq!(effective_config(&cli), PathBuf::from("/tmp/x.toml"));

        let cli = Cli::parse_from(["phprt", "-c", "", "list"]);
        assert_eq!(
            effective_config(&cli),
            PathBuf::from(".").join("phprt.toml")
        );
    }

    #[test]
    fn download_dir_follows_mode_defaults() {
        let root = Path::new("/proj");
        assert_eq!(
            effective_download_dir(None, None, root, false).unwrap(),
            None
        );
        assert_eq!(
            effective_download_dir(None, None, root, true).unwrap(),
            Some(layout::resolve_home("~/phprtdownloads").unwrap())
        );
        assert_eq!(
            effective_download_dir(Some(Path::new("/data/d")), None, root, false).unwrap(),
            Some(PathBuf::from("/data/d"))
        );
        assert_eq!(
            effective_download_dir(None, Some("d"), root, false).unwrap(),
            Some(PathBuf::from("/proj/d"))
        );
        assert_eq!(
            effective_download_dir(None, Some("/data/d"), root, true).unwrap(),
            Some(PathBuf::from("/data/d"))
        );
    }

    #[test]
    fn dirs_precedence_is_cli_then_config_then_default() {
        assert_eq!(
            effective_dir(Some(Path::new("/x")), "cfg", "build", "build_dir").unwrap(),
            "/x"
        );
        assert_eq!(
            effective_dir(None, "cfg", "build", "build_dir").unwrap(),
            "cfg"
        );
        assert_eq!(
            effective_dir(Some(Path::new("")), "", "build", "build_dir").unwrap(),
            "build"
        );
        assert!(effective_dir(Some(Path::new("../x")), "", "build", "build_dir").is_err());

        assert_eq!(
            effective_dir_opt(None, &Some("backups".into()), "backup_dir")
                .unwrap()
                .as_deref(),
            Some("backups")
        );
        assert_eq!(effective_dir_opt(None, &None, "backup_dir").unwrap(), None);
    }

    fn config_with(recipes: &str) -> config::Config {
        toml::from_str(&format!("recipes = '{recipes}'\n")).unwrap()
    }

    #[test]
    fn parallel_prefers_cli_then_config_then_default() {
        let mut config: config::Config = toml::from_str("parallel = 4\n").unwrap();
        assert_eq!(effective_parallel(Some(2), &config).unwrap(), 2);
        assert_eq!(effective_parallel(None, &config).unwrap(), 4);

        config.parallel = None;
        assert_eq!(
            effective_parallel(None, &config).unwrap(),
            config::default_parallel()
        );
        assert!(effective_parallel(Some(0), &config).is_err());
    }

    #[test]
    fn recipes_precedence_is_cli_then_config_then_default() {
        let root = Path::new("/proj");
        let cli = Path::new("/cli/recipes");
        assert_eq!(
            effective_recipes(Some(cli), root, &config_with("/cfg/recipes")),
            PathBuf::from("/cli/recipes")
        );
        assert_eq!(
            effective_recipes(None, root, &config_with("/cfg/recipes")),
            PathBuf::from("/cfg/recipes")
        );
        assert_eq!(
            effective_recipes(None, root, &config_with("recipes")),
            PathBuf::from("/proj/recipes")
        );
        assert_eq!(
            effective_recipes(Some(Path::new("")), root, &config_with("")),
            PathBuf::from("recipes")
        );
    }

    fn catalog() -> Catalog {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        Catalog::load(&dir.join("catalog.toml")).unwrap()
    }

    fn metas() -> Vec<RecipeMeta> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        recipe::load_meta(&dir, &catalog()).unwrap()
    }

    fn raw(name: &str, version: &str) -> RawComponent {
        RawComponent {
            name: name.to_owned(),
            version: Some(version.to_owned()),
        }
    }

    #[test]
    fn target_requires_product_and_series() {
        assert_eq!(
            split_target("php@8.5").unwrap(),
            ("php".to_owned(), "8.5".to_owned())
        );
        assert!(split_target("php").is_err());
        assert!(split_target("@8.5").is_err());
        assert!(split_target("php@").is_err());
    }

    #[test]
    fn install_accepts_only_one_target() {
        assert_eq!(
            single_target(&["php@8.5".to_owned()], "install").unwrap(),
            "php@8.5"
        );
        let err = single_target(&["php@8.5".to_owned(), "nginx@1.29".to_owned()], "install")
            .unwrap_err()
            .to_string();
        assert!(err.contains("只接受一个产品"), "{err}");
    }

    #[test]
    fn component_version_only_applies_to_versioned_components() {
        let pins = pins_from_cli(&catalog(), &[raw("swoole", "6.0")], &[], &metas()).unwrap();
        assert_eq!(pins["swoole"], lock::CliPin::Line("6.0".into()));

        let err = pins_from_cli(&catalog(), &[raw("openssl", "4.0")], &[], &metas())
            .unwrap_err()
            .to_string();
        assert!(err.contains("内置组件"), "{err}");
        assert!(err.contains("--dep openssl"), "{err}");

        let err = pins_from_cli(&catalog(), &[raw("curl", "8.0")], &[], &metas())
            .unwrap_err()
            .to_string();
        assert!(err.contains("内置组件"), "{err}");
    }

    #[test]
    fn dep_pin_only_accepts_dependency_recipes() {
        let pins = pins_from_cli(&catalog(), &[], &["openssl=4.0".to_owned()], &metas()).unwrap();
        assert_eq!(pins["openssl"], lock::CliPin::Line("4.0".into()));

        let err = pins_from_cli(&catalog(), &[], &["swoole=6.0".to_owned()], &metas())
            .unwrap_err()
            .to_string();
        assert!(err.contains("不是依赖库"), "{err}");
    }
    #[test]
    fn product_patch_stays_on_installed_version_until_update() {
        // 用固定夹具，不跟随 recipes/catalog.toml 的日常补丁更新。
        let catalog = Catalog::parse(
            r#"
[php]
"8.4" = [
    { version = "8.4.14", url = "https://example.com/8.4.14.tar.gz" },
    { version = "8.4.13", url = "https://example.com/8.4.13.tar.gz" },
    { version = "8.4.12", url = "https://example.com/8.4.12.tar.gz" },
]
"#,
            "fixture",
        )
        .unwrap();
        // 默认：已有 installed_version 就沿用它，不跟 catalog 当前补丁。
        assert_eq!(
            select_product_patch(&catalog, "php", "8.4", "8.4", false, "8.4.13", false).unwrap(),
            "8.4.13"
        );
        // 显式补丁和 update 才前进；没有安装记录时取 catalog 当前。
        assert_eq!(
            select_product_patch(&catalog, "php", "8.4", "8.4.14", true, "8.4.13", false).unwrap(),
            "8.4.14"
        );
        assert_eq!(
            select_product_patch(&catalog, "php", "8.4", "8.4", false, "8.4.13", true).unwrap(),
            "8.4.14"
        );
        assert_eq!(
            select_product_patch(&catalog, "php", "8.4", "8.4", false, "", false).unwrap(),
            "8.4.14"
        );
        let err = select_product_patch(&catalog, "php", "8.4", "8.4", false, "8.4.9", false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("state 里的 php 8.4.9"), "{err}");
    }

    #[test]
    fn scheme_command_renders_reusable_install_line() {
        let state = ProjectState {
            product: "php".into(),
            series: "8.5".into(),
            components: BTreeMap::from([
                (
                    "curl".to_owned(),
                    state::ComponentState {
                        mode: "static".into(),
                        deps: BTreeMap::from([("libcurl".into(), vec!["ssl".into()])]),
                        ..Default::default()
                    },
                ),
                (
                    "gd".to_owned(),
                    state::ComponentState {
                        mode: "static".into(),
                        features: vec!["jpeg".into()],
                        ..Default::default()
                    },
                ),
                (
                    "swoole".to_owned(),
                    state::ComponentState {
                        mode: "shared".into(),
                        ..Default::default()
                    },
                ),
                (
                    "zip".to_owned(),
                    state::ComponentState {
                        mode: "shared".into(),
                        deps: BTreeMap::from([("libzip".into(), vec!["encryption".into()])]),
                        ..Default::default()
                    },
                ),
            ]),
            bindings: BTreeMap::from([
                ("swoole".to_owned(), "6.2".to_owned()),
                ("openssl".to_owned(), "3.5".to_owned()),
            ]),
            ..ProjectState::default()
        };

        let line = marker::scheme_command(
            &state.product,
            &state.series,
            &state.as_selection(),
            &state.bindings,
            &crate::recipe::TargetSelection::new(),
            &|name| name == "openssl",
        );
        assert_eq!(
            line,
            "phprt install php@8.5 'curl=mode:static,dep:libcurl[ssl],gd=mode:static,feature:[jpeg],swoole=mode:shared,version:6.2,zip=mode:shared,dep:libzip[encryption]' --dep openssl=3.5"
        );
    }

    #[test]
    fn install_target_separates_product_from_scheme_list() {
        let (one, list) =
            install_target(&["php@8.5".to_owned(), "curl=mode:static,swoole".to_owned()]).unwrap();
        assert_eq!(one, "php@8.5");
        assert_eq!(list.as_deref(), Some("curl=mode:static,swoole"));

        let err = install_target(&["php@8.5".to_owned(), "nginx@1.29".to_owned()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("只接受一个产品"), "{err}");
    }

    #[test]
    fn install_skips_online_check_when_nothing_would_build() {
        let mut project = ProjectState {
            installed_version: "8.5.12".to_owned(),
            ..ProjectState::default()
        };
        project.components.insert(
            "curl".to_owned(),
            crate::state::ComponentState {
                line: String::new(),
                version: String::new(),
                mode: "static".to_owned(),
                installed: true,
                ..crate::state::ComponentState::default()
            },
        );
        project.deps.insert(
            "openssl".to_owned(),
            crate::state::DepState {
                line: "3.5".to_owned(),
                version: "3.5.9".to_owned(),
                features: Vec::new(),
            },
        );

        let product = Target {
            kind: crate::recipe::Kind::Product,
            name: "php".to_owned(),
            version: "8.5.12".to_owned(),
            ..Target::default()
        };
        let curl = Target {
            kind: crate::recipe::Kind::Component,
            name: "curl".to_owned(),
            ..Target::default()
        };
        let openssl = Target {
            kind: crate::recipe::Kind::Dep,
            name: "openssl".to_owned(),
            version: "3.5.9".to_owned(),
            ..Target::default()
        };

        let order = vec![&openssl, &curl, &product];
        let args = ComponentArgs::default();
        let targets = TargetArgs::default();
        assert!(!install_would_build(
            &project,
            "php",
            "8.5.12",
            &order,
            &Selection::default(),
            &args,
            &targets,
            &None,
            false,
            false,
        ));

        let mut newer = openssl.clone();
        newer.version = "3.5.10".to_owned();
        let newer_order = vec![&newer, &curl, &product];
        assert!(install_would_build(
            &project,
            "php",
            "8.5.12",
            &newer_order,
            &Selection::default(),
            &args,
            &targets,
            &None,
            false,
            false,
        ));

        assert!(install_would_build(
            &project,
            "php",
            "8.5.12",
            &order,
            &Selection::default(),
            &args,
            &targets,
            &Some("php@8.5".to_owned()),
            false,
            false,
        ));
        assert!(install_would_build(
            &project,
            "php",
            "8.5.12",
            &order,
            &Selection::default(),
            &args,
            &targets,
            &None,
            true,
            false,
        ));
    }

    #[test]
    fn refresh_flags_conflict_with_each_other_and_offline() {
        assert!(
            Cli::try_parse_from(["phprt", "install", "php@8.5", "--refresh", "--no-refresh"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from(["phprt", "install", "php@8.5", "--refresh", "--offline"]).is_err()
        );
    }

    #[test]
    fn catalog_publish_requires_out() {
        assert!(Cli::try_parse_from(["phprt", "catalog", "publish"]).is_err());
        let cli = Cli::try_parse_from(["phprt", "catalog", "publish", "--out", "dist"]).unwrap();
        assert!(matches!(
            cli.cmd,
            Cmd::Catalog {
                command: CatalogCmd::Publish { .. }
            }
        ));
    }

    #[test]
    fn remote_items_put_the_product_first_and_dedupe() {
        let catalog = Catalog::parse(
            r#"
[php]
"8.5" = [ { version = "8.5.12", url = "https://example.com/php-8.5.12.tar.xz", sha256 = "aa" } ]

[openssl]
"3.5" = [ { version = "3.5.9", url = "https://example.com/openssl-3.5.9.tar.gz", sha256 = "bb" } ]
"#,
            "fixture",
        )
        .unwrap();
        let product = Target {
            kind: crate::recipe::Kind::Product,
            name: "php".to_owned(),
            version: "8.5.12".to_owned(),
            ..Target::default()
        };
        let dep = Target {
            kind: crate::recipe::Kind::Dep,
            name: "openssl".to_owned(),
            version: "3.5.9".to_owned(),
            ..Target::default()
        };
        // 同名 bundled 组件和依赖库只取一次。
        let bundled = Target {
            kind: crate::recipe::Kind::Component,
            name: "openssl".to_owned(),
            ..Target::default()
        };
        let order = vec![&dep, &bundled, &product];
        let mut bindings = bind::Bindings::default();
        bindings
            .lines
            .insert("openssl".to_owned(), "3.5".to_owned());
        let items = remote_items(
            &order,
            &catalog,
            &bindings,
            &ProjectState::default(),
            "php",
            "8.5",
        );
        let names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, ["php", "openssl"]);
        assert_eq!(items[0].line, "8.5");
        assert_eq!(items[1].line, "3.5");
    }
}
