mod actions;
mod audit;
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
mod tools;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};

use crate::bundle::Bundle;
use crate::catalog::Catalog;
use crate::recipe::{Kind, RawComponent, RecipeMeta, Registry, Selection};
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

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
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
        #[command(flatten)]
        components: ComponentArgs,
    },
    /// 下载、解压、编译并安装；只接受一个产品
    Install {
        /// 产品@系列，如 php@8.5
        target: Vec<String>,
        #[command(flatten)]
        components: ComponentArgs,
        #[command(flatten)]
        download: DownloadArgs,
    },
    /// 更新：重新从 catalog 解析该产品线的最新补丁并构建
    Update {
        /// 产品@系列，如 php@8.5
        target: Vec<String>,
        #[command(flatten)]
        components: ComponentArgs,
        #[command(flatten)]
        download: DownloadArgs,
    },
    /// 把组件加入构建方案（待安装）
    Add {
        /// 产品@系列，如 php@8.5
        target: String,
        /// 组件 spec，如 swoole / 'swoole=mode:shared,version:6.2'
        component: String,
        /// 直接绑定依赖库的版本线：NAME=LINE，可重复
        #[arg(long = "dep", value_name = "NAME=LINE")]
        dep: Vec<String>,
    },
    /// 把组件移出构建方案
    Remove {
        /// 产品@系列，如 php@8.5
        target: String,
        /// 组件名
        name: String,
    },
    /// 对照上游选项清单，检查 recipe 是否漏掉或静默忽略了开关
    Audit {
        /// 产品@系列，如 php@8.5
        target: String,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
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

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

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

    let (target_spec, components, download, goal, json) = match &cli.cmd {
        Cmd::List { target, .. } => (
            target.clone().unwrap_or_default(),
            ComponentArgs::default(),
            None,
            None,
            false,
        ),
        Cmd::Graph { target } => (target.clone(), ComponentArgs::default(), None, None, false),
        Cmd::Plan {
            target,
            goal,
            json,
            components,
        } => (
            target.clone(),
            components.clone(),
            None,
            goal.clone(),
            *json,
        ),
        Cmd::Install {
            target,
            components,
            download,
        }
        | Cmd::Update {
            target,
            components,
            download,
        } => (
            single_target(target, "install")?,
            components.clone(),
            Some(download.clone()),
            None,
            false,
        ),
        Cmd::Add {
            target,
            component,
            dep,
        } => (
            target.clone(),
            ComponentArgs {
                component: vec![component.clone()],
                dep: dep.clone(),
                ..ComponentArgs::default()
            },
            None,
            None,
            false,
        ),
        Cmd::Remove { target, .. } => (target.clone(), ComponentArgs::default(), None, None, false),
        Cmd::Audit { target, json } => {
            (target.clone(), ComponentArgs::default(), None, None, *json)
        }
    };
    let (product_name, series) = split_target(&target_spec)?;

    let catalog = Catalog::load(&cli.recipes.join("catalog.toml"))
        .with_context(|| format!("加载 {}", cli.recipes.display()))?;
    let metas = recipe::load_meta(&cli.recipes, &catalog)?;
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
    let series = catalog.find_product_series(&product_name, &series)?;
    let version = catalog.product_version(&product_name, &series)?.to_owned();

    let bundle_path = cli
        .recipes
        .join("bundles")
        .join(&product_name)
        .join(format!("{series}.toml"));
    let bundle = Bundle::load(&bundle_path)?;
    bundle.check_product(&product_name)?;

    let state_file = state::state_path(&cli.root, &product_name, &series);
    let mut project = ProjectState::load(&state_file)?;
    project.check(&product_name, &series)?;

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

    let raw = recipe::preparse(&components.component)?;
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

    let mut cli_pins = pins_from_cli(&raw, &components.dep, &metas)?;

    // 首次方案从 bundle 的默认组件开始；已有 state 时以 state 为准。
    let seeded_defaults = !project.active;
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
    let mut removed_mode = None;
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
            removed_mode = project.components.get(name).map(|c| c.mode.clone());
            if !project.remove_component(name) {
                bail!("component:{name} 不在构建方案里");
            }
            selected.retain(|item| item != name);
            cli_pins.remove(name);
        }
        _ => {}
    }

    let mut pins = project.bindings.clone();
    pins.extend(cli_pins.clone());
    let bindings = bind::resolve(
        &catalog,
        &bundle,
        &metas,
        &product_name,
        &series,
        &selected,
        &pins,
    )?;
    for note in &bindings.notes {
        eprintln!("注意: {note}");
    }

    let ctx = bind::context(&product_name, &series, &version, &bindings);
    let mut reg = recipe::load_targets(
        &cli.recipes,
        &catalog,
        &product_name,
        &series,
        &metas,
        &bindings.lines,
        &ctx,
    )?;

    let default_sel = bundle::selection(&reg, &bundle, &series)?;
    let preset_sel = bundle::presets(&reg, &bundle, &series)?;
    if seeded_defaults {
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

    let cli_sel = Selection::parse(&reg, &components.component)?;
    let product_id = reg.product_any()?.id();

    match cli.cmd {
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
        Cmd::Install { .. } | Cmd::Update { .. } => {
            let names = scheme_names(&project, &raw, None);
            project.apply_scheme(&mut reg, &names)?;
            let sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
            let features = reg_features(&reg, &sel, &project);
            let order = plan::resolve(&reg, &product_id, &sel, &features)?;
            let sim = render::build(
                &reg,
                &order,
                &cli.root.display().to_string(),
                &sel,
                &features,
            )?;
            print!("{}", render::to_text(&sim));

            let download = download.expect("install/update 一定有下载参数");
            if !download.downloads {
                tools::check(reg.product_any()?, &order)?;
            }
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
            project.installed_version = version.clone();
            // install --component 只影响这次构建，不写进 state；
            // 持久化组件方案用 add / remove。
            let persist: Vec<String> = project.components.keys().cloned().collect();
            project.sync_selection(&reg, &sel, &persist)?;
            project.set_deps(&order, &features);
            project.set_installed_all();
            project.save(&state_file)?;
            println!();
            println!("已写入 {}", state_file.display());
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
        Cmd::Add { .. } => {
            let name = raw
                .first()
                .map(|item| item.name.clone())
                .ok_or_else(|| anyhow!("缺少组件名"))?;
            let added = project.add_component_with_preset(
                &reg,
                &name,
                preset_sel.per_component.get(&name),
            )?;
            let names = scheme_names(&project, &raw, None);
            project.apply_scheme(&mut reg, &names)?;
            let sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
            let features = reg_features(&reg, &sel, &project);
            let order = plan::resolve(&reg, &product_id, &sel, &features)?;

            project.bindings.extend(cli_pins.clone());
            project.installed_version = version.clone();
            project.sync_selection(&reg, &sel, &names)?;
            project.set_deps(&order, &features);
            project.save(&state_file)?;
            if added {
                println!("已加入组件 {name}（待安装）");
            } else {
                println!("组件 {name} 已在方案里");
            }
            println!("已写入 {}", state_file.display());
        }
        Cmd::Remove { name, .. } => {
            // 已经在解析绑定前从 project.components / selected 里移除。
            let names = scheme_names(&project, &raw, Some(&name));
            project.apply_scheme(&mut reg, &names)?;
            let mut sel = project.effective_selection(&reg, &cli_sel, &preset_sel, &names)?;
            sel.per_component.remove(&name);
            let features = reg_features(&reg, &sel, &project);
            let order = plan::resolve(&reg, &product_id, &sel, &features)?;

            project.bindings.extend(cli_pins.clone());
            project.installed_version = version.clone();
            project.sync_selection(&reg, &sel, &names)?;
            project.set_deps(&order, &features);
            let product = reg.product_any()?.clone();
            let component = reg.component(&name)?.clone();
            build::disable_component(
                &cli.root,
                &product,
                &component,
                removed_mode.as_deref().unwrap_or(&component.default_mode),
            )?;
            project.save(&state_file)?;
            println!("已移除组件 {name}");
            println!("已写入 {}", state_file.display());
        }
    }
    Ok(())
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
    let dir = root.join(".phprt");
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
    raw: &[RawComponent],
    deps: &[String],
    metas: &[RecipeMeta],
) -> Result<BTreeMap<String, String>> {
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
            pins.insert(component.name.clone(), version.clone());
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
        if !metas
            .iter()
            .any(|meta| meta.kind == Kind::Dep && meta.name == name)
        {
            bail!("--dep {name} 不是依赖库");
        }
        if pins.insert(name.to_owned(), line.to_owned()).is_some() {
            bail!("--dep 里重复指定了 {name}");
        }
    }
    Ok(pins)
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

    fn metas() -> Vec<RecipeMeta> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes");
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        recipe::load_meta(&dir, &catalog).unwrap()
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
        let pins = pins_from_cli(&[raw("swoole", "6.0")], &[], &metas()).unwrap();
        assert_eq!(pins["swoole"], "6.0");

        let err = pins_from_cli(&[raw("openssl", "4.0")], &[], &metas())
            .unwrap_err()
            .to_string();
        assert!(err.contains("内置组件"), "{err}");
        assert!(err.contains("--dep openssl"), "{err}");

        let err = pins_from_cli(&[raw("curl", "8.0")], &[], &metas())
            .unwrap_err()
            .to_string();
        assert!(err.contains("内置组件"), "{err}");
    }

    #[test]
    fn dep_pin_only_accepts_dependency_recipes() {
        let pins = pins_from_cli(&[], &["openssl=4.0".to_owned()], &metas()).unwrap();
        assert_eq!(pins["openssl"], "4.0");

        let err = pins_from_cli(&[], &["swoole=6.0".to_owned()], &metas())
            .unwrap_err()
            .to_string();
        assert!(err.contains("不是依赖库"), "{err}");
    }
}
