mod bind;
mod build;
mod bundle;
mod catalog;
mod config;
mod layout;
mod plan;
mod recipe;
mod render;
mod sources;
mod state;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};

use crate::bundle::Bundle;
use crate::catalog::Catalog;
use crate::recipe::{Kind, RawExt, Registry, Selection};
use crate::state::ProjectState;
use phprt_ui::Mode;

#[derive(Parser)]
#[command(
    name = "phprt",
    version,
    about = "phprt v3：Starlark 写 recipe，Rust 负责求解、下载、解压和构建"
)]
struct Cli {
    /// recipe 目录
    #[arg(long, default_value = "recipes", global = true)]
    recipes: PathBuf,

    /// 项目根目录；状态写在 <root>/.phprt 下
    #[arg(long, default_value = ".", global = true)]
    root: PathBuf,

    /// PHP 系列，如 8.5；install 使用位置参数，其他命令用这个
    #[arg(long = "php", value_name = "SERIES", global = true)]
    php: Option<String>,

    /// 扩展选择：名字[=link:模式][,dep:依赖[feature,...]][,feature:[名字,...]][,version:线]；可重复
    #[arg(long = "ext", value_name = "SPEC", global = true)]
    ext: Vec<String>,

    /// 直接绑定依赖库的版本线：NAME=LINE，可重复
    #[arg(long = "dep", value_name = "NAME=LINE", global = true)]
    dep: Vec<String>,

    /// 禁用当前 PHP 系列的默认扩展；可重复
    #[arg(long = "disable-default-ext", value_name = "NAME", global = true)]
    disable_default_ext: Vec<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 列出当前 PHP 系列下所有 recipe
    List,
    /// 打印依赖关系
    Graph,
    /// 求解并打印构建计划（只读）
    Plan {
        /// 目标，如 php:8.5.12 / ext:curl / dep:openssl；省略时取 php
        target: Option<String>,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
    },
    /// 下载、解压、编译并安装：必须指定 PHP 版本
    Install {
        /// PHP 系列或版本，如 8.5 / 8.5.12 / php:8.5.12
        php: Option<String>,
        #[command(flatten)]
        download: DownloadArgs,
    },
    /// 更新：重新从 catalog 解析当前线的最新补丁并构建
    Update {
        #[command(flatten)]
        download: DownloadArgs,
    },
    /// 扩展管理
    Ext {
        #[command(subcommand)]
        what: ExtCmd,
    },
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
    /// 进度显示方式
    #[arg(long, value_enum, default_value_t = Mode::Auto, value_name = "MODE")]
    ui: Mode,
}

#[derive(Subcommand)]
enum ExtCmd {
    /// 列出扩展
    List {
        /// 只列已安装
        #[arg(long, conflicts_with = "pending")]
        installed: bool,
        /// 只列已加入方案但未安装
        #[arg(long)]
        pending: bool,
    },
    /// 把扩展加入构建方案（待安装）
    Add { name: String },
    /// 从构建方案移除扩展
    Remove { name: String },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let catalog = Catalog::load(&cli.recipes.join("catalog.toml"))
        .with_context(|| format!("加载 {}", cli.recipes.display()))?;
    let series = resolve_series(&cli, &catalog)?;
    let php_version = catalog.php_version(&series)?.to_owned();

    let metas = recipe::load_meta(&cli.recipes, &catalog)?;
    let bundle_path = cli
        .recipes
        .join("bundles")
        .join(format!("php{series}.toml"));
    let bundle = Bundle::load(&bundle_path)?;
    bundle.check_series(&series)?;

    let state_file = state::state_path(&cli.root, &series);
    let mut project = ProjectState::load(&state_file)?;

    let php_meta = metas
        .iter()
        .find(|meta| meta.kind == Kind::Php && meta.series.as_deref() == Some(series.as_str()))
        .ok_or_else(|| anyhow!("没有找到 PHP {series} 的 recipe 元数据"))?;
    let builtin_exts = php_meta.builtin_exts.clone();

    let default_names: Vec<String> = bundle.default_exts.keys().cloned().collect();
    for name in &default_names {
        if !builtin_exts.iter().any(|item| item == name) {
            bail!("bundle php{series}.toml 的 default_exts 里 {name} 不是该 PHP 版本的内置扩展");
        }
        if !metas
            .iter()
            .any(|meta| meta.kind == Kind::Ext && &meta.name == name)
        {
            bail!("bundle php{series}.toml 的 default_exts 里 {name} 没有 ext recipe");
        }
    }

    let raw_exts = recipe::preparse(&cli.ext)?;
    let mut disabled_defaults = BTreeSet::new();
    for name in &cli.disable_default_ext {
        if !disabled_defaults.insert(name.clone()) {
            bail!("--disable-default-ext 重复指定了 {name}");
        }
        if !default_names.iter().any(|item| item == name) {
            let available = if default_names.is_empty() {
                "无".to_owned()
            } else {
                default_names.join(", ")
            };
            bail!("--disable-default-ext {name} 不是 PHP {series} 的默认扩展（默认：{available}）");
        }
        if raw_exts.iter().any(|raw| &raw.name == name) {
            bail!("--ext 和 --disable-default-ext 同时指定了 {name}");
        }
    }
    if !disabled_defaults.is_empty()
        && !matches!(
            &cli.cmd,
            Cmd::Install { .. } | Cmd::Update { .. } | Cmd::Plan { .. }
        )
    {
        bail!("--disable-default-ext 只支持 install / update / plan");
    }

    let mut cli_pins = pins_from_cli(&raw_exts, &cli.dep)?;

    // 首次方案从 bundle 的默认扩展开始；已有 state 时以 state 为准。
    let seeded_defaults = !project.active;
    if seeded_defaults {
        for name in &default_names {
            if disabled_defaults.contains(name) {
                continue;
            }
            project.ext.insert(
                name.clone(),
                state::ExtState {
                    link: String::new(),
                    installed: false,
                    ..state::ExtState::default()
                },
            );
        }
        project.active = true;
    } else {
        for name in &disabled_defaults {
            project.remove_ext(name);
        }
    }

    let mut selected: Vec<String> = project.ext.keys().cloned().collect();
    for raw in &raw_exts {
        if !selected.contains(&raw.name) {
            selected.push(raw.name.clone());
        }
    }

    // add / remove 在解析绑定之前调整方案，避免给已移除的扩展做校验。
    match &cli.cmd {
        Cmd::Ext {
            what: ExtCmd::Add { name },
        } => {
            if !metas
                .iter()
                .any(|meta| meta.kind == Kind::Ext && &meta.name == name)
            {
                bail!("未知扩展 ext:{name}");
            }
            if !selected.contains(name) {
                selected.push(name.clone());
            }
        }
        Cmd::Ext {
            what: ExtCmd::Remove { name },
        } => {
            if !project.remove_ext(name) {
                bail!("ext:{name} 不在构建方案里");
            }
            selected.retain(|item| item != name);
            cli_pins.remove(name);
        }
        _ => {}
    }

    let mut pins = project.bindings.clone();
    pins.extend(cli_pins.clone());
    let bindings = bind::resolve(&catalog, &bundle, &metas, &series, &selected, &pins)?;
    for note in &bindings.notes {
        eprintln!("注意: {note}");
    }

    let ctx = bind::context(&series, &php_version, &bindings);
    let mut reg = recipe::load_targets(
        &cli.recipes,
        &catalog,
        &series,
        &metas,
        &bindings.lines,
        &ctx,
        &builtin_exts,
    )?;

    let default_sel = bundle::selection(&reg, &bundle, &series)?;
    if seeded_defaults {
        for (name, spec) in &default_sel.per_ext {
            let Some(entry) = project.ext.get_mut(name) else {
                continue;
            };
            if entry.link.is_empty()
                && let Some(mode) = spec.link
            {
                entry.link = mode.as_str().to_owned();
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

    let cli_sel = Selection::parse(&reg, &cli.ext)?;
    let php_id = format!("php:{php_version}");

    match cli.cmd {
        Cmd::List => {
            println!("series {series}（PHP {php_version}）");
            print!("{}", render::list_text(&reg));
        }
        Cmd::Graph => {
            let names = scheme_names(&project, &raw_exts, None);
            project.apply_scheme(&mut reg, &names)?;
            let sel = project.effective_selection(&reg, &cli_sel, &names)?;
            let features = project.merged_features(sel.features(&reg));
            print!("{}", render::graph_text(&reg, &sel, &features)?);
        }
        Cmd::Plan { target, json } => {
            let names = scheme_names(&project, &raw_exts, None);
            project.apply_scheme(&mut reg, &names)?;
            let sel = project.effective_selection(&reg, &cli_sel, &names)?;
            let features = project.merged_features(sel.features(&reg));
            let root = plan_root(&reg, &series, &php_version, target.as_deref())?;
            let order = plan::resolve(&reg, &root, &sel, &features)?;
            let sim = render::build(
                &reg,
                &order,
                &cli.root.display().to_string(),
                &sel,
                &features,
            )?;
            if json {
                println!("{}", render::to_json(&sim)?);
            } else {
                print!("{}", render::to_text(&sim));
            }
        }
        Cmd::Install { download, .. } | Cmd::Update { download } => {
            let names = scheme_names(&project, &raw_exts, None);
            project.apply_scheme(&mut reg, &names)?;
            let sel = project.effective_selection(&reg, &cli_sel, &names)?;
            let features = project.merged_features(sel.features(&reg));
            let order = plan::resolve(&reg, &php_id, &sel, &features)?;
            let sim = render::build(
                &reg,
                &order,
                &cli.root.display().to_string(),
                &sel,
                &features,
            )?;
            print!("{}", render::to_text(&sim));

            let cache = cli.root.join(".phprt").join("downloads");
            let sources = sources::collect(&order, &cli.root, &cache)?;
            let config = config::Config::load(&cli.recipes.join(config::FILE_NAME))?;
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
            phprt_download::run(&sources, &options)?;

            if download.downloads {
                println!();
                println!("源码已下载到 {}（未解压）", cache.display());
                return Ok(());
            }

            phprt_extract::run(&sources, download.ui)?;

            build::run(
                &reg,
                &order,
                &sel,
                &features,
                &build::Options {
                    root: &cli.root,
                    recipes: &cli.recipes,
                    ui: download.ui,
                },
            )?;

            project.bindings.extend(cli_pins.clone());
            project.php = php_version.clone();
            project.sync_selection(&reg, &sel, &names)?;
            project.set_deps(&order, &features);
            project.set_installed_all();
            project.save(&state_file)?;
            println!();
            println!("已写入 {}", state_file.display());
        }
        Cmd::Ext { what } => match what {
            ExtCmd::List { installed, pending } => {
                print!(
                    "{}",
                    render::ext_list_text(&reg, &project, installed, pending)
                );
            }
            ExtCmd::Add { name } => {
                let added = project.add_ext(&reg, &name)?;
                let names = scheme_names(&project, &raw_exts, None);
                project.apply_scheme(&mut reg, &names)?;
                let sel = project.effective_selection(&reg, &cli_sel, &names)?;
                let features = project.merged_features(sel.features(&reg));
                let order = plan::resolve(&reg, &php_id, &sel, &features)?;

                project.bindings.extend(cli_pins.clone());
                project.php = php_version.clone();
                project.sync_selection(&reg, &sel, &names)?;
                project.set_deps(&order, &features);
                project.save(&state_file)?;
                if added {
                    println!("已加入扩展 {name}（待安装）");
                } else {
                    println!("扩展 {name} 已在方案里");
                }
                println!("已写入 {}", state_file.display());
            }
            ExtCmd::Remove { name } => {
                // 已经在解析绑定前从 project.ext / selected 里移除。
                let names = scheme_names(&project, &raw_exts, Some(&name));
                project.apply_scheme(&mut reg, &names)?;
                let mut sel = project.effective_selection(&reg, &cli_sel, &names)?;
                sel.per_ext.remove(&name);
                let features = sel.features(&reg);
                let order = plan::resolve(&reg, &php_id, &sel, &features)?;

                project.bindings.extend(cli_pins.clone());
                project.php = php_version.clone();
                project.sync_selection(&reg, &sel, &names)?;
                project.set_deps(&order, &features);
                build::disable_ext(&cli.root, &php_version, reg.ext(&name)?)?;
                project.save(&state_file)?;
                println!("已移除扩展 {name}");
                println!("已写入 {}", state_file.display());
            }
        },
    }
    Ok(())
}

fn resolve_series(cli: &Cli, catalog: &Catalog) -> Result<String> {
    if let Cmd::Install { php, .. } = &cli.cmd {
        let spec = php.as_deref().ok_or_else(|| {
            anyhow!(
                "请指定 PHP 版本，例如：phprt install 8.5（可用：{}）",
                catalog.php_series().join(", ")
            )
        })?;
        let series = catalog.find_php_series(spec)?;
        if let Some(flag) = &cli.php {
            let other = catalog.find_php_series(flag)?;
            if other != series {
                bail!("install 的 PHP 版本与 --php 不一致");
            }
        }
        return Ok(series);
    }
    if let Some(flag) = &cli.php {
        return catalog.find_php_series(flag);
    }
    if matches!(cli.cmd, Cmd::List) {
        let found = scan_state_series(&cli.root)?;
        return match found.len() {
            1 => Ok(found[0].clone()),
            _ => catalog.highest_php_series(),
        };
    }
    if let Some(series) = infer_state_series(&cli.root)? {
        return Ok(series);
    }
    bail!("请用 --php 指定 PHP 系列，例如：phprt --php 8.5 plan")
}

fn scan_state_series(root: &Path) -> Result<Vec<String>> {
    let dir = root.join(".phprt");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("读不到 {}", dir.display()))? {
        let path = entry?.path();
        if !path.is_dir() || !path.join("state.json").is_file() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|s| s.to_str())
            && let Some(series) = name.strip_prefix("php-")
        {
            found.push(series.to_owned());
        }
    }
    found.sort();
    Ok(found)
}

fn infer_state_series(root: &Path) -> Result<Option<String>> {
    let found = scan_state_series(root)?;
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.into_iter().next()),
        _ => bail!("存在多个 PHP 系列（{}），请用 --php 指定", found.join(", ")),
    }
}

fn pins_from_cli(raw: &[RawExt], deps: &[String]) -> Result<BTreeMap<String, String>> {
    let mut pins = BTreeMap::new();
    for ext in raw {
        if let Some(version) = &ext.version {
            pins.insert(ext.name.clone(), version.clone());
        }
    }
    for spec in deps {
        let (name, value) = spec
            .find(['=', ':'])
            .map(|i| (&spec[..i], &spec[i + 1..]))
            .ok_or_else(|| anyhow!("--dep {spec} 写法不合法，应为 NAME=LINE"))?;
        let line = value.strip_prefix("version:").unwrap_or(value);
        if name.is_empty() || line.is_empty() {
            bail!("--dep {spec} 写法不合法，应为 NAME=LINE");
        }
        if pins.insert(name.to_owned(), line.to_owned()).is_some() {
            bail!("--dep 里重复指定了 {name}");
        }
    }
    Ok(pins)
}

fn scheme_names(project: &ProjectState, raw: &[RawExt], removed: Option<&str>) -> Vec<String> {
    let cli_names: Vec<String> = raw.iter().map(|ext| ext.name.clone()).collect();
    let mut names = project.scheme_with(&cli_names);
    if let Some(removed) = removed {
        names.retain(|name| name != removed);
    }
    names
}

fn plan_root(
    reg: &Registry,
    series: &str,
    php_version: &str,
    target: Option<&str>,
) -> Result<String> {
    let php_id = format!("php:{php_version}");
    let Some(target) = target else {
        return Ok(php_id);
    };
    if target == series || target == php_version || target == php_id {
        return Ok(php_id);
    }
    if let Some(rest) = target.strip_prefix("php:") {
        if rest == php_version || rest == series {
            return Ok(php_id);
        }
        bail!("当前加载的是 PHP {series}（{php_version}），不能 plan php:{rest}");
    }
    reg.resolve_name(target)
}
