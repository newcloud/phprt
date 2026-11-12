mod plan;
mod recipe;
mod render;
mod state;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::recipe::{Kind, Registry, Selection};
use crate::state::ProjectState;

#[derive(Parser)]
#[command(
    name = "phprt",
    version,
    about = "phprt v3 最小原型：Starlark 写 recipe，Rust 只做求解与模拟"
)]
struct Cli {
    /// recipe 目录
    #[arg(long, default_value = "recipes", global = true)]
    recipes: PathBuf,

    /// 项目根目录；状态写在 <root>/.phprt 下
    #[arg(long, default_value = ".", global = true)]
    root: PathBuf,

    /// 扩展选择：名字[=link:模式][,sub:名字[feature,feature]]；可重复
    #[arg(long = "ext", value_name = "SPEC", global = true)]
    ext: Vec<String>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 列出所有 recipe
    List,
    /// 打印依赖关系
    Graph,
    /// 求解并打印模拟的构建计划（只读）
    Plan {
        /// 目标，如 php:8.5.12 / ext:curl / dep:openssl；省略时取 php
        target: Option<String>,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
    },
    /// 模拟安装：输出计划、写状态并标记已安装
    Install,
    /// 模拟更新：重新求解、写状态并标记已安装
    Update,
    /// 扩展管理
    Ext {
        #[command(subcommand)]
        what: ExtCmd,
    },
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
    let mut reg =
        Registry::load(&cli.recipes).with_context(|| format!("加载 {}", cli.recipes.display()))?;

    let php_version = reg.php()?.version.clone();
    let series = state::series_of(&php_version);
    let state_file = state::state_path(&cli.root, &series);
    let mut project = ProjectState::load(&state_file)?;
    project.validate(&reg)?;

    match cli.cmd {
        Cmd::List => print!("{}", render::list_text(&reg)),
        Cmd::Graph => {
            project.apply_to_registry(&mut reg)?;
            let sel = project.effective_selection(&reg, &Selection::parse(&reg, &cli.ext)?)?;
            let features = project.merged_features(sel.features(&reg));
            print!("{}", render::graph_text(&reg, &sel, &features)?);
        }
        Cmd::Plan { target, json } => {
            project.apply_to_registry(&mut reg)?;
            let sel = project.effective_selection(&reg, &Selection::parse(&reg, &cli.ext)?)?;
            let features = project.merged_features(sel.features(&reg));
            let root = target.unwrap_or_else(|| default_php(&reg));
            let order = plan::resolve(&reg, &root, &sel, &features)?;
            let root_display = cli.root.display().to_string();
            let sim = render::build(&reg, &order, &root_display, &sel, &features)?;
            if json {
                println!("{}", render::to_json(&sim)?);
            } else {
                print!("{}", render::to_text(&sim));
            }
        }
        Cmd::Install | Cmd::Update => {
            project.ensure_seeded(&reg)?;
            project.apply_to_registry(&mut reg)?;
            let sel = project.effective_selection(&reg, &Selection::parse(&reg, &cli.ext)?)?;
            let features = project.merged_features(sel.features(&reg));
            let order = plan::resolve(&reg, &default_php(&reg), &sel, &features)?;
            let sim = render::build(
                &reg,
                &order,
                &cli.root.display().to_string(),
                &sel,
                &features,
            )?;
            print!("{}", render::to_text(&sim));

            project.sync_selection(&reg, &sel)?;
            project.set_deps(&order, &features);
            project.set_installed_all();
            project.php = reg.php()?.version.clone();
            project.save(&state_file)?;
            println!();
            println!("已写入 {}（模拟）", state_file.display());
        }
        Cmd::Ext { what } => match what {
            ExtCmd::List { installed, pending } => {
                print!(
                    "{}",
                    render::ext_list_text(&reg, &project, installed, pending)
                );
            }
            ExtCmd::Add { name } => {
                reg.ext(&name)?;
                project.ensure_seeded(&reg)?;
                let added = project.add_ext(&reg, &name)?;
                project.apply_to_registry(&mut reg)?;
                let sel = project.effective_selection(&reg, &Selection::parse(&reg, &cli.ext)?)?;
                let features = project.merged_features(sel.features(&reg));
                let order = plan::resolve(&reg, &default_php(&reg), &sel, &features)?;

                project.sync_selection(&reg, &sel)?;
                project.set_deps(&order, &features);
                project.php = reg.php()?.version.clone();
                project.save(&state_file)?;
                if added {
                    println!("已加入扩展 {name}（待安装）");
                } else {
                    println!("扩展 {name} 已在方案里");
                }
                println!("已写入 {}（模拟）", state_file.display());
            }
            ExtCmd::Remove { name } => {
                project.ensure_seeded(&reg)?;
                if !project.remove_ext(&name) {
                    bail!("ext:{name} 不在构建方案里");
                }
                project.apply_to_registry(&mut reg)?;
                let mut sel =
                    project.effective_selection(&reg, &Selection::parse(&reg, &cli.ext)?)?;
                // 被移除的扩展不再贡献 feature，也不接收本次命令行选择。
                sel.per_ext.remove(&name);
                let features = sel.features(&reg);
                let order = plan::resolve(&reg, &default_php(&reg), &sel, &features)?;

                project.sync_selection(&reg, &sel)?;
                project.set_deps(&order, &features);
                project.php = reg.php()?.version.clone();
                project.save(&state_file)?;
                println!("已移除扩展 {name}（模拟卸载）");
                println!("已写入 {}（模拟）", state_file.display());
            }
        },
    }
    Ok(())
}

fn default_php(reg: &Registry) -> String {
    reg.of_kind(Kind::Php)
        .next()
        .map(|t| t.id())
        .unwrap_or_else(|| "php".to_owned())
}
