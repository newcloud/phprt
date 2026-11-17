use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use phprt_build::{Cmd, Step};
use phprt_ui::Mode;

use crate::actions;
use crate::layout;
use crate::marker;
use crate::recipe::{Kind, Registry, Selection, Strategy, Target};
use crate::render::{self, Ctx};

pub struct Options<'a> {
    pub root: &'a Path,
    pub recipes: &'a Path,
    pub ui: Mode,
    /// 方案指纹和可复制的 install 命令，写进完成标记。
    pub scheme: &'a marker::Scheme,
    /// 是否按产品规则备份被改写的登记文件。
    pub backup: bool,
}

pub fn run(
    reg: &Registry,
    order: &[&Target],
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
    opts: &Options<'_>,
) -> Result<()> {
    let root = fs::canonicalize(opts.root)
        .with_context(|| format!("读不到项目根 {}", opts.root.display()))?;
    let product = reg.product_any()?;
    let tree_path = crate::layout::tree_path(&root, &product.name, &product.version);
    let tree = tree_path.display().to_string();
    let product_build = root.join(layout::build_dir_of(product));
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let vendor = fs::canonicalize(opts.recipes.join("../vendor/perl")).ok();
    let host = HostEnv::from_process();
    let mut dep_sigs: BTreeMap<String, String> = BTreeMap::new();

    let mut steps = Vec::new();
    let mut component_artifacts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for target in order {
        let mode = if target.kind == Kind::Component {
            Some(sel.mode_of(target))
        } else {
            None
        };
        let strategy = match (&mode, target.kind) {
            (Some(mode), Kind::Component) => Some(reg.product_mode(target, mode)?.strategy_kind()?),
            _ => None,
        };
        if strategy == Some(Strategy::Fold) {
            continue;
        }

        let prefix = match target.kind {
            Kind::Dep => dep_prefix(&tree_path, target),
            _ => tree_path.clone(),
        };
        let work = work_dir(&root, target, product, strategy, mode.as_deref());
        let deps = link_closure(reg, sel, features, target)?;
        let link_prefixes: Vec<PathBuf> = deps
            .iter()
            .filter_map(|name| reg.get(&format!("dep:{name}")))
            .map(|dep| dep_prefix(&tree_path, dep))
            .collect();
        let tool_prefixes: Vec<PathBuf> = target
            .tools
            .iter()
            .filter_map(|name| reg.get(&format!("dep:{name}")))
            .map(|dep| dep_prefix(&tree_path, dep))
            .collect();

        let mut env = build_env(
            &tree,
            &product.profile.runtime.lib_dir,
            &link_prefixes,
            &tool_prefixes,
            jobs,
            vendor.as_deref(),
        );
        let node_ctx = ctx(&tree, &work, &prefix);
        if target.kind != Kind::Dep {
            env.extend(profile_env(product, target, &node_ctx, order)?);
        }
        if target.kind == Kind::Component {
            env.extend(component_env(reg, target, &node_ctx)?);
        }
        if target.kind == Kind::Product {
            for name in &target.components {
                let component = reg.component(name)?;
                let component_mode = sel.mode_of(component);
                if reg
                    .product_mode(component, &component_mode)?
                    .strategy_kind()?
                    == Strategy::Fold
                {
                    let extra = component_env(reg, component, &node_ctx)?;
                    for (key, value) in extra {
                        if let Some(previous) = env.get(&key)
                            && previous != &value
                        {
                            bail!(
                                "组件 {} 的 {key} 与已有环境变量冲突（{previous} / {value}）",
                                component.name
                            );
                        }
                        env.insert(key, value);
                    }
                }
            }
        }

        let (commands, artifacts) = match target.kind {
            Kind::Dep => {
                let args =
                    render::dep_configure(reg, target, features.get(&target.name), &node_ctx)?;
                (
                    program_commands(args, &target.make, &node_ctx, reg)?,
                    artifacts_expanded(&target.artifacts, target),
                )
            }
            Kind::Product => {
                let mut args = target
                    .configure
                    .iter()
                    .map(|arg| render::expand(arg, &node_ctx, reg))
                    .collect::<Result<Vec<_>>>()?;
                args.extend(render::product_configure_flags(
                    reg, target, sel, &node_ctx,
                )?);
                let mut commands = Vec::new();
                if !args.is_empty() {
                    // 产品 recipe 的 configure 可以写 "./configure" 作为第一项，
                    // 也可以只写参数；两种情况都执行 ./configure。
                    let program = if args[0] == "./configure" {
                        args.remove(0)
                    } else {
                        "./configure".to_owned()
                    };
                    commands.push(Cmd::Program { program, args });
                }
                for line in &target.make {
                    commands.push(Cmd::Shell(render::expand(line, &node_ctx, reg)?));
                }
                let mut artifacts = artifacts_expanded(&target.artifacts, target);
                for name in &target.components {
                    let component = reg.component(name)?;
                    let component_mode = sel.mode_of(component);
                    let def = reg.product_mode(component, &component_mode)?;
                    if def.strategy_kind()? == Strategy::Fold {
                        artifacts.extend(artifacts_expanded(&def.artifacts, component));
                    }
                }
                (commands, artifacts)
            }
            Kind::Component => {
                let mode_name = mode.clone().unwrap_or_default();
                let def = reg.product_mode(target, &mode_name)?;
                let flags = render::component_configure(reg, target, sel, &node_ctx)?;
                let commands = component_commands(target, def, &flags, &node_ctx, reg)?;
                let artifacts = if target.artifacts.is_empty() {
                    artifacts_expanded(&def.artifacts, target)
                } else {
                    artifacts_expanded(&target.artifacts, target)
                };
                component_artifacts.insert(target.name.clone(), artifacts.clone());
                (commands, artifacts)
            }
        };

        let artifacts: Vec<PathBuf> = artifacts.into_iter().map(|rel| prefix.join(rel)).collect();

        let copy_from = match (target.kind, target.source.as_str(), strategy) {
            (Kind::Component, "bundled", Some(Strategy::Standalone)) => {
                Some(product_build.join(render::bundled_source_path(product, target)?))
            }
            _ => None,
        };
        let copy_sig = copy_from.as_deref().map(source_sig).transpose()?;
        let sig = signature(
            reg,
            target,
            &SigInputs {
                strategy,
                commands: &commands,
                env: &env,
                artifacts: &artifacts,
                copy_from: copy_sig.as_deref(),
                dep_sigs: &dep_sigs,
                deps: &deps,
            },
        )?;
        dep_sigs.insert(target.name.clone(), sig.clone());

        // 签名只用工具生成的确定性环境；宿主的 PATH / CFLAGS 等在这里才接上，
        // 换 shell 或上 CI 不会让整棵树重编。
        add_host_env(&mut env, &host);

        steps.push(Step {
            tag: format!(
                "[{}] {} {}",
                match target.kind {
                    Kind::Product => "product",
                    Kind::Dep => "deps",
                    Kind::Component => "component",
                },
                target.name,
                target.version
            ),
            cwd: work.clone(),
            copy_from,
            commands,
            env,
            artifacts,
            sig,
            log: root.join("build/logs").join(format!(
                "{}-{}.log",
                target.kind.as_str(),
                if target.name == target.version {
                    target.name.clone()
                } else {
                    format!("{}-{}", target.name, target.version)
                }
            )),
        });
    }

    phprt_build::run(&steps, &phprt_build::Options { ui: opts.ui })?;

    let tree = PathBuf::from(&tree);
    deploy_libs(reg, sel, features, product, order, &tree)?;
    actions::run(
        reg,
        product,
        sel,
        &tree,
        &actions::Options {
            root: opts.root,
            backup: opts.backup,
        },
    )?;
    verify(reg, product, &tree)?;
    let target = format!("{}@{}", product.name, product.series);
    write_entry(&root, product, opts.recipes, &target)?;
    // 整条流水线（含产品校验和入口）都成功了才写完成标记，且是最后一步。
    let installed = marker::InstalledTree {
        version: product.version.clone(),
        scheme: opts.scheme.digest.clone(),
        scheme_cmd: opts.scheme.command.clone(),
        components: component_facts(reg, product, sel, &tree, &component_artifacts)?,
    };
    marker::write(&root, &product.name, &installed)?;
    Ok(())
}

/// 把产物模式（可含一个 `*`）展开成实际文件，相对安装树。
/// 一个都没匹配到就保留模式本身，让“缺少产物”的检查能把它报出来。
fn concrete_artifacts(tree: &Path, patterns: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for pattern in patterns {
        let parts: Vec<&str> = pattern.split('/').collect();
        let mut found = Vec::new();
        walk_glob(tree, &parts, &mut found);
        if found.is_empty() {
            out.push(pattern.clone());
            continue;
        }
        for path in found {
            if let Ok(relative) = path.strip_prefix(tree) {
                out.push(relative.display().to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 每个组件在这棵树里留下的产物和 ini。删组件、核对 ini 有没有被改都靠它。
fn component_facts(
    reg: &Registry,
    product: &Target,
    sel: &Selection,
    tree: &Path,
    standalone: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<marker::ComponentFacts>> {
    let mut out = Vec::new();
    for name in &product.components {
        let component = reg.component(name)?;
        let mode = sel.mode_of(component);
        let def = reg.product_mode(component, &mode)?;
        let artifacts = match def.strategy_kind()? {
            // fold 组件的产物算在产品产物里，按 profile mode 的声明取。
            Strategy::Fold => artifacts_expanded(&def.artifacts, component),
            Strategy::Standalone => standalone.get(name).cloned().unwrap_or_else(|| {
                if component.artifacts.is_empty() {
                    artifacts_expanded(&def.artifacts, component)
                } else {
                    artifacts_expanded(&component.artifacts, component)
                }
            }),
        };
        let ini = product
            .profile
            .register
            .get(&mode)
            .and_then(|register| product.profile.register_dir(register))
            .filter(|_| !component.ini.is_empty())
            .map(|dir| format!("{dir}/{}", component.ini))
            .filter(|relative| tree.join(relative).is_file())
            .unwrap_or_default();
        let ini_hash = if ini.is_empty() {
            String::new()
        } else {
            marker::file_digest(&tree.join(&ini))?
        };
        out.push(marker::ComponentFacts {
            name: name.clone(),
            mode,
            ini,
            ini_hash,
            artifacts: concrete_artifacts(tree, &artifacts),
        });
    }
    Ok(out)
}

fn ctx<'a>(tree: &'a str, build: &Path, prefix: &Path) -> Ctx<'a> {
    Ctx {
        tree,
        build: build.display().to_string(),
        prefix: prefix.display().to_string(),
    }
}

/// standalone 组件的命令序列：先组件自己的 prepare，再 profile mode 的 steps。
/// 顺序即声明顺序，写进构建签名。
fn component_commands(
    target: &Target,
    def: &crate::recipe::ProfileMode,
    flags: &[String],
    node_ctx: &Ctx<'_>,
    reg: &Registry,
) -> Result<Vec<Cmd>> {
    let mut commands = Vec::new();
    for line in &target.prepare {
        commands.push(Cmd::Shell(render::expand(line, node_ctx, reg)?));
    }
    for template in &def.steps {
        let line = template
            .replace("{name}", &target.name)
            .replace("{product}", node_ctx.tree)
            .replace("{flags}", &flags.join(" "))
            .replace("{build}", &node_ctx.build)
            .replace("{prefix}", &node_ctx.prefix);
        commands.push(Cmd::Shell(render::expand(&line, node_ctx, reg)?));
    }
    Ok(commands)
}

fn program_commands(
    mut configure: Vec<String>,
    make: &[String],
    ctx: &Ctx<'_>,
    reg: &Registry,
) -> Result<Vec<Cmd>> {
    let mut commands = Vec::new();
    if !configure.is_empty() {
        let program = configure.remove(0);
        commands.push(Cmd::Program {
            program,
            args: configure,
        });
    }
    for line in make {
        commands.push(Cmd::Shell(render::expand(line, ctx, reg)?));
    }
    Ok(commands)
}

fn artifacts_expanded(patterns: &[String], target: &Target) -> Vec<String> {
    patterns
        .iter()
        .map(|pattern| pattern.replace("{name}", &target.name))
        .collect()
}

/// 签名输入：除节点身份外的全部构建输入，打包传递避免参数表过长。
struct SigInputs<'a> {
    strategy: Option<Strategy>,
    commands: &'a [Cmd],
    env: &'a BTreeMap<String, String>,
    artifacts: &'a [PathBuf],
    copy_from: Option<&'a str>,
    dep_sigs: &'a BTreeMap<String, String>,
    deps: &'a BTreeSet<String>,
}

/// 一次构建的完整输入：身份、源码、命令、环境、产物、源码拷贝内容、
/// 以及依赖闭包里每个 dep 的版本和构建签名。
/// 依赖只按实际闭包计入，不把整个 Registry 的 recipe 塞进签名。
fn signature(reg: &Registry, target: &Target, inputs: &SigInputs<'_>) -> Result<String> {
    let SigInputs {
        strategy,
        commands,
        env,
        artifacts,
        copy_from,
        dep_sigs,
        deps,
    } = inputs;
    let mut out = String::new();
    out.push_str(&format!("id={}\n", target.id()));
    out.push_str(&format!("version={}\n", target.version));
    out.push_str(&format!("url={}\n", target.url));
    out.push_str(&format!("sha256={}\n", target.sha256));
    for tool in &target.host_tools {
        out.push_str(&format!("host-tool={} {}\n", tool.name, tool.min));
    }
    out.push_str(&format!(
        "strategy={}\n",
        strategy.map(|s| s.as_str()).unwrap_or("")
    ));
    for cmd in *commands {
        match cmd {
            Cmd::Program { program, args } => {
                out.push_str(&format!("program={program}\n"));
                for arg in args {
                    out.push_str(&format!("arg={arg}\n"));
                }
            }
            Cmd::Shell(line) => out.push_str(&format!("shell={line}\n")),
        }
    }
    for (key, value) in *env {
        out.push_str(&format!("env={key}={value}\n"));
    }
    for artifact in *artifacts {
        out.push_str(&format!("artifact={}\n", artifact.display()));
    }
    if let Some(hash) = *copy_from {
        out.push_str(&format!("copy-from={hash}\n"));
    }
    for name in *deps {
        let dep = reg
            .get(&format!("dep:{name}"))
            .with_context(|| format!("签名里找不到依赖 {name}"))?;
        out.push_str(&format!("dep={} {}\n", name, dep.version));
        if let Some(sig) = dep_sigs.get(name) {
            out.push_str(&format!("dep-sig={sig}\n"));
        }
    }
    Ok(out)
}

/// copy_from 源码目录的内容指纹：相对路径 + 内容 hash，软链记目标。
/// 目录按路径排序，结果与遍历顺序无关。
fn source_sig(dir: &Path) -> Result<String> {
    let mut entries: BTreeMap<String, String> = BTreeMap::new();
    walk_source(dir, dir, &mut entries)?;
    let mut text = String::new();
    for (rel, digest) in &entries {
        text.push_str(&format!("{rel} {digest}\n"));
    }
    Ok(sig_hash(&text))
}

fn walk_source(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let rel = entry.path().strip_prefix(base)?.display().to_string();
        let meta = fs::symlink_metadata(entry.path())?;
        if meta.file_type().is_symlink() {
            out.insert(
                rel,
                format!("link:{}", fs::read_link(entry.path())?.display()),
            );
        } else if meta.is_dir() {
            walk_source(base, &entry.path(), out)?;
        } else {
            out.insert(
                rel,
                sig_hash(&String::from_utf8_lossy(&fs::read(entry.path())?)),
            );
        }
    }
    Ok(())
}

fn sig_hash(text: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(text.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn dep_prefix(tree: &Path, dep: &Target) -> PathBuf {
    tree.join("deps")
        .join(format!("{}-{}", dep.name, dep.version))
}

fn work_dir(
    root: &Path,
    target: &Target,
    product: &Target,
    strategy: Option<Strategy>,
    mode: Option<&str>,
) -> PathBuf {
    let _ = mode;
    match (target.kind, target.source.as_str(), strategy) {
        (Kind::Component, "bundled", Some(Strategy::Standalone)) => root
            .join(crate::layout::build_dir_name())
            .join(&target.name)
            .join(format!("{}-{}", product.name, product.version)),
        _ => root.join(layout::build_dir_of(target)),
    }
}

fn link_closure(
    reg: &Registry,
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
    target: &Target,
) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    match target.kind {
        Kind::Dep => walk_dep(reg, features, &target.name, &mut out),
        Kind::Component => {
            for need in &target.needs {
                walk_dep(reg, features, need, &mut out);
            }
            for need in sel.feature_needs(target) {
                walk_dep(reg, features, need, &mut out);
            }
        }
        Kind::Product => {
            for need in &target.needs {
                walk_dep(reg, features, need, &mut out);
            }
            for name in &target.components {
                let component = reg.component(name)?;
                let mode = sel.mode_of(component);
                if reg.product_mode(component, &mode)?.strategy_kind()? != Strategy::Fold {
                    continue;
                }
                for need in &component.needs {
                    walk_dep(reg, features, need, &mut out);
                }
                for need in sel.feature_needs(component) {
                    walk_dep(reg, features, need, &mut out);
                }
            }
        }
    }
    Ok(out)
}

fn walk_dep(
    reg: &Registry,
    features: &BTreeMap<String, Vec<String>>,
    name: &str,
    out: &mut BTreeSet<String>,
) {
    if !out.insert(name.to_owned()) {
        return;
    }
    let Some(dep) = reg.get(&format!("dep:{name}")) else {
        return;
    };
    for need in &dep.needs {
        walk_dep(reg, features, need, out);
    }
    if let Some(enabled) = features.get(name) {
        for feature in enabled {
            if let Some(def) = dep.features.get(feature) {
                for need in &def.needs {
                    walk_dep(reg, features, need, out);
                }
            }
        }
    }
}

fn build_env(
    tree: &str,
    runtime_lib_dir: &str,
    deps: &[PathBuf],
    tools: &[PathBuf],
    jobs: usize,
    vendor: Option<&Path>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    let mut cpp = String::new();
    let mut ld = String::new();
    let mut pc: Vec<String> = Vec::new();
    let mut lib_path: Vec<String> = Vec::new();
    if !runtime_lib_dir.is_empty() {
        lib_path.push(Path::new(tree).join(runtime_lib_dir).display().to_string());
    }
    for dep in deps {
        cpp.push_str(&format!("-I{}/include ", dep.display()));
        ld.push_str(&format!("-L{}/lib ", dep.display()));
        pc.push(format!("{}/lib/pkgconfig", dep.display()));
        pc.push(format!("{}/share/pkgconfig", dep.display()));
        lib_path.push(format!("{}/lib", dep.display()));
        lib_path.push(format!("{}/lib64", dep.display()));
    }
    let mut path: Vec<String> = vec![format!("{tree}/bin")];
    for tool in tools {
        path.push(tool.join("bin").display().to_string());
    }

    env.insert("CPPFLAGS".to_owned(), cpp.trim_end().to_owned());
    env.insert("LDFLAGS".to_owned(), ld.trim_end().to_owned());
    env.insert("PKG_CONFIG_PATH".to_owned(), pc.join(":"));
    env.insert("LD_LIBRARY_PATH".to_owned(), lib_path.join(":"));
    env.insert("PATH".to_owned(), path.join(":"));
    env.insert("CFLAGS".to_owned(), "-O2".to_owned());
    env.insert("CXXFLAGS".to_owned(), "-O2".to_owned());
    env.insert("MAKEFLAGS".to_owned(), format!("-j{jobs}"));
    if let Some(vendor) = vendor {
        env.insert("PERL5LIB".to_owned(), vendor.display().to_string());
    }
    env
}

/// 宿主环境里会被接进构建环境的部分。执行时才追加，
/// 签名只算上面 `build_env` 的确定性结果：换 shell、上 CI 不该导致全量重编。
#[derive(Default)]
struct HostEnv {
    path: Option<String>,
    cppflags: Option<String>,
    ldflags: Option<String>,
    cflags: Option<String>,
    cxxflags: Option<String>,
    perl5lib: Option<String>,
}

impl HostEnv {
    fn from_process() -> Self {
        fn get(key: &str) -> Option<String> {
            std::env::var(key).ok().filter(|value| !value.is_empty())
        }
        Self {
            path: std::env::var_os("PATH")
                .map(|value| value.to_string_lossy().into_owned())
                .filter(|value| !value.is_empty()),
            cppflags: get("CPPFLAGS"),
            ldflags: get("LDFLAGS"),
            cflags: get("CFLAGS"),
            cxxflags: get("CXXFLAGS"),
            perl5lib: get("PERL5LIB"),
        }
    }
}

/// 把宿主的值接到工具生成的值后面，等价于 shell 里的 `VAR=base $VAR`。
fn add_host_env(env: &mut BTreeMap<String, String>, host: &HostEnv) {
    fn append(env: &mut BTreeMap<String, String>, key: &str, extra: &Option<String>) {
        let Some(extra) = extra else {
            return;
        };
        let base = env.get(key).cloned().unwrap_or_default();
        let base = base.trim_end();
        let value = if base.is_empty() {
            extra.clone()
        } else {
            format!("{base} {extra}")
        };
        env.insert(key.to_owned(), value);
    }

    append(env, "CPPFLAGS", &host.cppflags);
    append(env, "LDFLAGS", &host.ldflags);
    append(env, "CFLAGS", &host.cflags);
    append(env, "CXXFLAGS", &host.cxxflags);
    if let Some(path) = &host.path {
        let base = env.get("PATH").cloned().unwrap_or_default();
        let value = if base.is_empty() {
            path.clone()
        } else {
            format!("{base}:{path}")
        };
        env.insert("PATH".to_owned(), value);
    }
    if let Some(perl) = &host.perl5lib
        && let Some(base) = env.get("PERL5LIB").cloned()
    {
        env.insert("PERL5LIB".to_owned(), format!("{base}:{perl}"));
    }
}

/// 产品 profile 声明的环境变量；引用了不在本次构建里的工具时跳过。
/// 同一个变量只注入 `tools` 里包含对应工具 dep 的节点，
/// 例如 PHP profile 的 `{tool:autoconf}` 不会进 nginx 或 dep 节点。
fn profile_env(
    product: &Target,
    node: &Target,
    ctx: &Ctx<'_>,
    order: &[&Target],
) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (key, value) in &product.profile.env {
        let Some(expanded) = expand_tool(value, ctx, order, &node.tools)? else {
            continue;
        };
        out.insert(key.clone(), expanded);
    }
    Ok(out)
}

fn component_env(
    reg: &Registry,
    component: &Target,
    ctx: &Ctx<'_>,
) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (key, value) in &component.env {
        out.insert(key.clone(), render::expand(value, ctx, reg)?);
    }
    Ok(out)
}

fn expand_tool(
    template: &str,
    ctx: &Ctx<'_>,
    order: &[&Target],
    node_tools: &[String],
) -> Result<Option<String>> {
    let mut value = template.replace("{tree}", ctx.tree);
    while let Some(start) = value.find("{tool:") {
        let after = &value[start + 6..];
        let Some(end) = after.find('}') else {
            break;
        };
        let name = &after[..end];
        if !node_tools.iter().any(|tool| tool == name) {
            return Ok(None);
        }
        let Some(dep) = order.iter().find(|t| t.kind == Kind::Dep && t.name == name) else {
            return Ok(None);
        };
        let prefix = format!("{}/deps/{}-{}", ctx.tree, dep.name, dep.version);
        value.replace_range(start..start + 6 + end + 1, &prefix);
    }
    Ok(Some(value))
}

fn deploy_libs(
    reg: &Registry,
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
    product: &Target,
    order: &[&Target],
    tree: &Path,
) -> Result<()> {
    let lib_dir = &product.profile.runtime.lib_dir;
    if lib_dir.is_empty() {
        return Ok(());
    }
    let flat = tree.join(lib_dir);
    if flat.is_dir() {
        fs::remove_dir_all(&flat).with_context(|| format!("清不了 {}", flat.display()))?;
    }
    fs::create_dir_all(&flat).with_context(|| format!("建不了 {}", flat.display()))?;

    let runtime = runtime_deps(reg, sel, features, order)?;
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for target in order
        .iter()
        .filter(|t| t.kind == Kind::Dep && runtime.contains(&t.name))
    {
        let prefix = dep_prefix(tree, target);
        for dir in [prefix.join("lib"), prefix.join("lib64")] {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if !name_str.contains(".so") {
                    continue;
                }
                let name = name_str.into_owned();
                if let Some(previous) = seen.insert(name.clone(), target.name.clone()) {
                    bail!("运行时库重名: {name}（{} 和 {}）", previous, target.name);
                }
                let from = entry.path();
                let to = flat.join(&name);
                let meta = fs::symlink_metadata(&from)?;
                if meta.file_type().is_symlink() {
                    let link = fs::read_link(&from)?;
                    std::os::unix::fs::symlink(link, &to)
                        .with_context(|| format!("建不了软链 {}", to.display()))?;
                } else {
                    fs::copy(&from, &to).with_context(|| {
                        format!("复制不了 {} -> {}", from.display(), to.display())
                    })?;
                }
            }
        }
    }

    for entry in fs::read_dir(&flat)? {
        let entry = entry?;
        let path = entry.path();
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            continue;
        }
        patchelf_rpath(&path, "$ORIGIN")?;
    }

    for (pattern, rpath) in &product.profile.runtime.rpath {
        for path in glob_paths(tree, pattern) {
            if !is_elf(&path) {
                continue;
            }
            patchelf_rpath(&path, rpath)?;
        }
    }
    Ok(())
}

/// 产品运行时依赖：只沿 `needs` / 已启用 feature 的 `needs` 求闭包，
/// `tools` 这类构建期依赖不进入运行时库收集。
fn runtime_deps(
    reg: &Registry,
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
    order: &[&Target],
) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for target in order.iter().filter(|t| t.kind != Kind::Dep) {
        out.extend(link_closure(reg, sel, features, target)?);
    }
    Ok(out)
}

/// 在安装树里展开 `bin/*`、`lib/php/extensions/*/*.so` 这类相对 glob。
fn glob_paths(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let parts: Vec<&str> = pattern.split('/').filter(|p| !p.is_empty()).collect();
    let mut out = Vec::new();
    walk_glob(root, &parts, &mut out);
    out
}

fn walk_glob(dir: &Path, parts: &[&str], out: &mut Vec<PathBuf>) {
    if parts.is_empty() {
        if dir.exists() {
            out.push(dir.to_path_buf());
        }
        return;
    }
    let (first, rest) = (parts[0], &parts[1..]);
    if first.contains('*') {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if glob_match(first, &name) {
                walk_glob(&entry.path(), rest, out);
            }
        }
        return;
    }
    let next = dir.join(first);
    if !next.exists() {
        return;
    }
    walk_glob(&next, rest, out);
}

fn glob_match(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, suffix)) => {
            name.len() >= prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix)
        }
        None => pattern == name,
    }
}

fn patchelf_rpath(path: &Path, rpath: &str) -> Result<()> {
    let status = Command::new("patchelf")
        .arg("--set-rpath")
        .arg(rpath)
        .arg("--force-rpath")
        .arg(path)
        .status()
        .with_context(|| format!("跑不起来 patchelf（{}）", path.display()))?;
    if !status.success() {
        bail!("patchelf 设置 rpath 失败: {}", path.display());
    }
    Ok(())
}

fn is_elf(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(b"\x7fELF")
}

fn verify(reg: &Registry, product: &Target, tree: &Path) -> Result<()> {
    if product.profile.verify.is_empty() {
        return Ok(());
    }
    let mut argv = Vec::new();
    for arg in &product.profile.verify {
        argv.push(arg.replace("{tree}", &tree.display().to_string()));
    }
    let program = argv.remove(0);
    let output = Command::new(&program)
        .args(&argv)
        .output()
        .with_context(|| format!("跑不起来 {}", program))?;
    if !output.status.success() {
        bail!(
            "{} {} 失败:\n{}",
            program,
            argv.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    if product.profile.verify_modules.is_empty() {
        return Ok(());
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
            "{} 失败:\n{}",
            command,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let loaded = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let mut missing = Vec::new();
    for name in &product.components {
        let component = reg.component(name)?;
        if component.modules.is_empty() {
            continue;
        }
        for module in &component.modules {
            if !loaded
                .lines()
                .any(|line| line.trim().eq_ignore_ascii_case(module))
            {
                missing.push(format!("{} ({})", module, name));
            }
        }
    }
    if !missing.is_empty() {
        bail!(
            "{} 启动后缺少组件模块:\n  {}\n{}",
            product.name,
            missing.join("\n  "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn write_entry(root: &Path, product: &Target, recipes: &Path, target: &str) -> Result<()> {
    if product.profile.entry.is_empty() || crate::layout::is_global() {
        return Ok(());
    }
    let template_path = recipes
        .join("products")
        .join(&product.name)
        .join(&product.profile.entry_template);
    let template = fs::read_to_string(&template_path)
        .with_context(|| format!("读不到入口模板 {}", template_path.display()))?;
    let name = product.profile.entry.replace("{series}", &product.series);
    let content = template
        .replace("@@TREE@@", &format!("{}-{}", product.name, product.version))
        .replace("@@TARGET@@", target);
    write_script(&root.join(&name), &content)?;

    if let Some(base) = name.strip_suffix(&format!("-{}", product.series))
        && !base.is_empty()
        && only_series(root, product)
    {
        write_script(&root.join(base), &content)?;
    }
    Ok(())
}

fn write_script(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content).with_context(|| format!("写不了 {}", path.display()))?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

/// 项目里是否只有当前产品的这一个系列（用来决定要不要写不带系列的入口别名）。
fn only_series(root: &Path, product: &Target) -> bool {
    let Ok(entries) = fs::read_dir(crate::layout::internal_dir(root)) else {
        return true;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_prefix(&format!("{}-", product.name)) else {
            continue;
        };
        if rest != product.series && entry.path().join("state.json").is_file() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{Profile, ProfileMode};

    fn blank() -> Target {
        Target {
            kind: Kind::Dep,
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

    fn dep(name: &str, needs: &[&str]) -> Target {
        Target {
            kind: Kind::Dep,
            name: name.into(),
            version: "1".into(),
            url: format!("https://example.com/{name}.tar.gz"),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            ..blank()
        }
    }

    fn bundled(name: &str, needs: &[&str], tools: &[&str]) -> Target {
        Target {
            kind: Kind::Component,
            name: name.into(),
            product: "php".into(),
            source: "bundled".into(),
            modes: vec!["static".into(), "shared".into()],
            default_mode: "static".into(),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            tools: tools.iter().map(|s| s.to_string()).collect(),
            modules: vec![name.into()],
            ini: format!("20-{name}.ini"),
            ..blank()
        }
    }

    fn external(name: &str, needs: &[&str], tools: &[&str]) -> Target {
        Target {
            kind: Kind::Component,
            name: name.into(),
            product: "php".into(),
            source: "external".into(),
            modes: vec!["shared".into()],
            default_mode: "shared".into(),
            version: "1.0".into(),
            url: format!("https://pecl.php.net/get/{name}-1.0.tgz"),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            tools: tools.iter().map(|s| s.to_string()).collect(),
            modules: vec![name.into()],
            ini: format!("20-{name}.ini"),
            ..blank()
        }
    }

    fn profile() -> Profile {
        let mut profile = Profile::default();
        profile.modes.insert(
            "static".into(),
            ProfileMode {
                strategy: "fold".into(),
                ..Default::default()
            },
        );
        profile.modes.insert(
            "shared".into(),
            ProfileMode {
                strategy: "standalone".into(),
                ..Default::default()
            },
        );
        profile.defaults.insert("bundled".into(), "static".into());
        profile.defaults.insert("external".into(), "shared".into());
        profile
    }

    fn product(components: &[&str]) -> Target {
        Target {
            kind: Kind::Product,
            name: "php".into(),
            version: "8.5.12".into(),
            url: "https://example.com/php.tar.gz".into(),
            series: "8.5".into(),
            bundled: vec!["curl".into()],
            components: components.iter().map(|s| s.to_string()).collect(),
            profile: profile(),
            ..blank()
        }
    }

    fn fixture() -> Registry {
        Registry::from_targets(vec![
            dep("openssl", &[]),
            dep("libcurl", &["openssl"]),
            dep("pcre2", &[]),
            dep("autoconf", &[]),
            bundled("curl", &["libcurl"], &[]),
            external("swoole", &["pcre2"], &["autoconf"]),
            product(&["curl", "swoole"]),
        ])
        .unwrap()
    }

    #[test]
    fn build_env_uses_profile_runtime_lib_dir() {
        let env = build_env("/tree", "lib/custom", &[], &[], 4, None);
        assert_eq!(
            env["LD_LIBRARY_PATH"].split(':').next(),
            Some("/tree/lib/custom")
        );
        assert_eq!(env["MAKEFLAGS"], "-j4");
        assert!(!env.contains_key("PHP_AUTOCONF"));
        assert!(!env.contains_key("PHP_AUTOHEADER"));

        let deps = vec![PathBuf::from("/deps/zlib")];
        let env = build_env("/tree", "", &deps, &[], 4, None);
        assert!(!env["LD_LIBRARY_PATH"].contains("/tree/"));
        assert!(env["LD_LIBRARY_PATH"].starts_with("/deps/zlib/lib"));
    }

    #[test]
    fn signature_env_has_no_host_values() {
        let env = build_env("/tree", "", &[], &[], 4, None);
        assert_eq!(env["PATH"], "/tree/bin");
        assert_eq!(env["CFLAGS"], "-O2");
        assert_eq!(env["CXXFLAGS"], "-O2");
        assert_eq!(env["CPPFLAGS"], "");
        assert_eq!(env["LDFLAGS"], "");
        assert!(!env.contains_key("PERL5LIB"));
    }

    #[test]
    fn host_env_is_added_for_execution_only() {
        let mut env = build_env("/tree", "", &[], &[], 4, None);
        let host = HostEnv {
            path: Some("/host/bin".into()),
            cflags: Some("-march=native".into()),
            cxxflags: Some("-fPIC".into()),
            cppflags: Some("-DHOST".into()),
            ldflags: Some("-L/host/lib".into()),
            perl5lib: Some("/host/perl".into()),
        };
        add_host_env(&mut env, &host);
        assert_eq!(env["CFLAGS"], "-O2 -march=native");
        assert_eq!(env["CXXFLAGS"], "-O2 -fPIC");
        assert_eq!(env["CPPFLAGS"], "-DHOST");
        assert_eq!(env["LDFLAGS"], "-L/host/lib");
        assert_eq!(env["PATH"], "/tree/bin:/host/bin");
        // build_env 没给 PERL5LIB 时，宿主的也不补：和原来一致。
        assert!(!env.contains_key("PERL5LIB"));

        let deps = vec![PathBuf::from("/deps/zlib")];
        let mut env = build_env("/tree", "", &deps, &[], 4, Some(Path::new("/vendor/perl")));
        add_host_env(&mut env, &host);
        assert_eq!(env["CPPFLAGS"], "-I/deps/zlib/include -DHOST");
        assert_eq!(env["PERL5LIB"], "/vendor/perl:/host/perl");
    }

    #[test]
    fn runtime_deps_follow_needs_but_not_tools() {
        let reg = fixture();
        let sel = Selection::default();
        let features = BTreeMap::new();
        let order: Vec<&Target> = [
            "dep:openssl",
            "dep:libcurl",
            "dep:pcre2",
            "dep:autoconf",
            "component:swoole",
            "product:php@8.5.12",
        ]
        .iter()
        .map(|id| reg.get(id).unwrap())
        .collect();

        let runtime = runtime_deps(&reg, &sel, &features, &order).unwrap();
        let expected: BTreeSet<String> = ["libcurl", "openssl", "pcre2"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        assert_eq!(runtime, expected);
        assert!(order.iter().any(|target| target.name == "autoconf"));
        assert!(!runtime.contains("autoconf"));
    }

    #[test]
    fn runtime_deps_include_enabled_feature_needs_and_transitive_deps() {
        let mut curl = external("curl", &[], &[]);
        curl.features.insert(
            "ssl".into(),
            crate::recipe::Feature {
                needs: vec!["libssl".into()],
                ..Default::default()
            },
        );
        let mut prod = product(&["curl"]);
        prod.bundled.clear();
        let reg = Registry::from_targets(vec![
            dep("libssl", &["libcrypto"]),
            dep("libcrypto", &[]),
            curl,
            prod,
        ])
        .unwrap();

        let features = BTreeMap::new();
        let order: Vec<&Target> = [
            "dep:libssl",
            "dep:libcrypto",
            "component:curl",
            "product:php@8.5.12",
        ]
        .iter()
        .map(|id| reg.get(id).unwrap())
        .collect();

        let mut enabled = Selection::default();
        let spec = crate::recipe::ComponentSpec {
            features: Some(["ssl".to_owned()].into_iter().collect()),
            ..Default::default()
        };
        enabled.per_component.insert("curl".into(), spec);

        let runtime = runtime_deps(&reg, &enabled, &features, &order).unwrap();
        let expected: BTreeSet<String> = ["libcrypto", "libssl"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        assert_eq!(runtime, expected);

        // 没有选中 feature 时，feature 的 needs 不进运行时闭包。
        let runtime = runtime_deps(&reg, &Selection::default(), &features, &order).unwrap();
        assert!(runtime.is_empty(), "{runtime:?}");
    }

    #[test]
    fn dep_used_as_tool_and_need_is_deployed() {
        let mut prod = product(&["swoole", "build-helper"]);
        prod.bundled.clear();
        let reg = Registry::from_targets(vec![
            dep("autoconf", &[]),
            dep("pcre2", &[]),
            external("swoole", &["pcre2"], &["autoconf"]),
            external("build-helper", &["autoconf"], &[]),
            prod,
        ])
        .unwrap();
        let sel = Selection::default();
        let features = BTreeMap::new();
        let order: Vec<&Target> = [
            "dep:autoconf",
            "dep:pcre2",
            "component:swoole",
            "component:build-helper",
            "product:php@8.5.12",
        ]
        .iter()
        .map(|id| reg.get(id).unwrap())
        .collect();

        let runtime = runtime_deps(&reg, &sel, &features, &order).unwrap();
        assert!(runtime.contains("autoconf"), "{runtime:?}");
        assert!(runtime.contains("pcre2"), "{runtime:?}");
    }

    #[test]
    fn deploy_libs_copies_runtime_deps_but_not_build_tools() {
        let root = std::env::temp_dir().join(format!("phprt-deploy-libs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let tree = root.join("tree");
        let runtime_lib = tree.join("deps/demo-1/lib");
        fs::create_dir_all(&runtime_lib).unwrap();

        let source = root.join("demo.c");
        fs::write(&source, "int demo(void) { return 0; }\n").unwrap();
        let real = runtime_lib.join("libdemo.so.1.0");
        let status = Command::new("cc")
            .args(["-shared", "-fPIC", "-Wl,-soname,libdemo.so.1", "-o"])
            .arg(&real)
            .arg(&source)
            .status()
            .unwrap();
        assert!(status.success());
        std::os::unix::fs::symlink("libdemo.so.1.0", runtime_lib.join("libdemo.so.1")).unwrap();
        std::os::unix::fs::symlink("libdemo.so.1", runtime_lib.join("libdemo.so")).unwrap();

        let tool_lib = tree.join("deps/toolonly-1/lib");
        fs::create_dir_all(&tool_lib).unwrap();
        fs::write(tool_lib.join("libtoolonly.so"), b"build tool\n").unwrap();

        let mut prod = product(&["sample"]);
        prod.profile.runtime.lib_dir = "lib/phprt".into();
        prod.bundled.clear();
        let reg = Registry::from_targets(vec![
            dep("demo", &[]),
            dep("toolonly", &[]),
            external("sample", &["demo"], &["toolonly"]),
            prod,
        ])
        .unwrap();
        let order: Vec<&Target> = [
            "dep:demo",
            "dep:toolonly",
            "component:sample",
            "product:php@8.5.12",
        ]
        .iter()
        .map(|id| reg.get(id).unwrap())
        .collect();

        deploy_libs(
            &reg,
            &Selection::default(),
            &BTreeMap::new(),
            reg.get("product:php@8.5.12").unwrap(),
            &order,
            &tree,
        )
        .unwrap();

        let flat = tree.join("lib/phprt");
        assert!(flat.join("libdemo.so.1.0").is_file());
        assert_eq!(
            fs::read_link(flat.join("libdemo.so")).unwrap(),
            Path::new("libdemo.so.1")
        );
        assert_eq!(
            fs::read_link(flat.join("libdemo.so.1")).unwrap(),
            Path::new("libdemo.so.1.0")
        );
        assert!(!flat.join("libtoolonly.so").exists());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_env_tool_vars_follow_node_tools() {
        let mut product = product(&[]);
        product
            .profile
            .env
            .insert("PHP_AUTOCONF".into(), "{tool:autoconf}/bin/autoconf".into());
        let autoconf = dep("autoconf", &[]);
        let order = vec![&autoconf];
        let ctx = Ctx {
            tree: "/tree",
            build: "/build".into(),
            prefix: "/tree".into(),
        };

        let mut node = external("swoole", &[], &["autoconf"]);
        let env = profile_env(&product, &node, &ctx, &order).unwrap();
        assert_eq!(
            env.get("PHP_AUTOCONF").map(String::as_str),
            Some("/tree/deps/autoconf-1/bin/autoconf")
        );

        node.tools.clear();
        let env = profile_env(&product, &node, &ctx, &order).unwrap();
        assert!(!env.contains_key("PHP_AUTOCONF"));
    }

    fn component_def(steps: &[&str]) -> crate::recipe::ProfileMode {
        crate::recipe::ProfileMode {
            strategy: "standalone".into(),
            steps: steps.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn node_ctx() -> Ctx<'static> {
        Ctx {
            tree: "/tree",
            build: "/build".into(),
            prefix: "/tree".into(),
        }
    }

    fn sig_of(
        reg: &Registry,
        target: &Target,
        commands: &[Cmd],
        copy_from: Option<&str>,
        dep_sigs: &BTreeMap<String, String>,
        deps: &BTreeSet<String>,
    ) -> String {
        signature(
            reg,
            target,
            &SigInputs {
                strategy: Some(Strategy::Standalone),
                commands,
                env: &BTreeMap::new(),
                artifacts: &[],
                copy_from,
                dep_sigs,
                deps,
            },
        )
        .unwrap()
    }

    #[test]
    fn signature_changes_when_prepare_changes() {
        let reg = fixture();
        let def = component_def(&["phpize", "make"]);
        let ctx = node_ctx();

        let mut first = bundled("curl", &["libcurl"], &[]);
        first.prepare = vec!["echo one".into()];
        let commands = component_commands(&first, &def, &[], &ctx, &reg).unwrap();
        let sig_a = sig_of(
            &reg,
            &first,
            &commands,
            None,
            &BTreeMap::new(),
            &BTreeSet::new(),
        );

        let mut second = first.clone();
        second.prepare = vec!["echo two".into()];
        let commands = component_commands(&second, &def, &[], &ctx, &reg).unwrap();
        let sig_b = sig_of(
            &reg,
            &second,
            &commands,
            None,
            &BTreeMap::new(),
            &BTreeSet::new(),
        );

        assert_ne!(sig_a, sig_b);
        assert!(sig_a.contains("shell=echo one"));
        assert!(sig_b.contains("shell=echo two"));
    }

    #[test]
    fn signature_changes_when_copy_from_content_changes() {
        let root = std::env::temp_dir().join(format!("phprt-sig-copy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);

        let mut sources = Vec::new();
        for content in ["first", "second"] {
            let dir = root.join(format!("src-{content}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("foo.c"), content).unwrap();
            sources.push(dir);
        }
        let hash_a = source_sig(&sources[0]).unwrap();
        let hash_b = source_sig(&sources[1]).unwrap();
        assert_ne!(hash_a, hash_b);

        let reg = fixture();
        let target = bundled("curl", &["libcurl"], &[]);
        let sig_a = sig_of(
            &reg,
            &target,
            &[],
            Some(&hash_a),
            &BTreeMap::new(),
            &BTreeSet::new(),
        );
        let sig_b = sig_of(
            &reg,
            &target,
            &[],
            Some(&hash_b),
            &BTreeMap::new(),
            &BTreeSet::new(),
        );
        assert_ne!(sig_a, sig_b);
        assert!(sig_a.contains(&format!("copy-from={hash_a}")));

        // 同样的内容重复计算，签名稳定。
        assert_eq!(hash_a, source_sig(&sources[0]).unwrap());

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn signature_follows_dep_version_and_dep_signature() {
        let sel = Selection::default();
        let features = BTreeMap::new();

        let reg = Registry::from_targets(vec![
            dep("libcurl", &[]),
            bundled("curl", &["libcurl"], &[]),
            product(&["curl"]),
        ])
        .unwrap();
        let component = reg.get("component:curl").unwrap();
        let deps = link_closure(&reg, &sel, &features, component).unwrap();
        let ctx = node_ctx();
        let def = reg.product_mode(component, "shared").unwrap();
        let commands = component_commands(component, def, &[], &ctx, &reg).unwrap();
        let sig_v1 = sig_of(&reg, component, &commands, None, &BTreeMap::new(), &deps);
        assert!(sig_v1.contains("dep=libcurl 1\n"));

        // dep 版本变化，下游签名变化。
        let reg = Registry::from_targets(vec![
            dep("libcurl-2", &[]),
            bundled("curl", &["libcurl-2"], &[]),
            product(&["curl"]),
        ])
        .unwrap();
        let component = reg.get("component:curl").unwrap();
        let deps = link_closure(&reg, &sel, &features, component).unwrap();
        let def = reg.product_mode(component, "shared").unwrap();
        let commands = component_commands(component, def, &[], &ctx, &reg).unwrap();
        let sig_v2 = sig_of(&reg, component, &commands, None, &BTreeMap::new(), &deps);
        assert_ne!(sig_v1, sig_v2);

        // dep 构建签名变化（版本没变），下游签名也要变化。
        let reg = Registry::from_targets(vec![
            dep("libcurl", &[]),
            bundled("curl", &["libcurl"], &[]),
            product(&["curl"]),
        ])
        .unwrap();
        let component = reg.get("component:curl").unwrap();
        let deps = link_closure(&reg, &sel, &features, component).unwrap();
        let ctx = node_ctx();
        let def = reg.product_mode(component, "shared").unwrap();
        let commands = component_commands(component, def, &[], &ctx, &reg).unwrap();
        let mut dep_sigs = BTreeMap::new();
        dep_sigs.insert("libcurl".into(), "old-dep-sig".into());
        let sig_old = sig_of(&reg, component, &commands, None, &dep_sigs, &deps);
        dep_sigs.insert("libcurl".into(), "new-dep-sig".into());
        let sig_new = sig_of(&reg, component, &commands, None, &dep_sigs, &deps);
        assert_ne!(sig_old, sig_new);
        assert!(sig_new.contains("dep-sig=new-dep-sig"));
    }

    #[test]
    fn unselected_feature_does_not_change_signature() {
        let sel = Selection::default();
        let features = BTreeMap::new();
        let ctx = node_ctx();

        let mut with_feature = external("curl", &["libcurl"], &[]);
        with_feature.features.insert(
            "extra".into(),
            crate::recipe::Feature {
                needs: vec!["libextra".into()],
                ..Default::default()
            },
        );
        let mut prod = product(&["curl"]);
        prod.bundled.clear();
        let reg = Registry::from_targets(vec![
            dep("libcurl", &[]),
            dep("libextra", &[]),
            with_feature,
            prod,
        ])
        .unwrap();
        let component = reg.get("component:curl").unwrap();
        let deps = link_closure(&reg, &sel, &features, component).unwrap();
        let def = reg.product_mode(component, "shared").unwrap();
        let commands = component_commands(component, def, &[], &ctx, &reg).unwrap();
        let sig_with = sig_of(&reg, component, &commands, None, &BTreeMap::new(), &deps);

        // 同样的组件，但没有那个未启用的 feature 定义。
        let plain = external("curl", &["libcurl"], &[]);
        let mut prod = product(&["curl"]);
        prod.bundled.clear();
        let reg =
            Registry::from_targets(vec![dep("libcurl", &[]), dep("libextra", &[]), plain, prod])
                .unwrap();
        let component = reg.get("component:curl").unwrap();
        let deps = link_closure(&reg, &sel, &features, component).unwrap();
        let def = reg.product_mode(component, "shared").unwrap();
        let commands = component_commands(component, def, &[], &ctx, &reg).unwrap();
        let sig_without = sig_of(&reg, component, &commands, None, &BTreeMap::new(), &deps);

        assert_eq!(sig_with, sig_without);
    }

    #[test]
    fn same_inputs_produce_same_signature() {
        let reg = fixture();
        let target = bundled("curl", &["libcurl"], &[]);
        let commands = vec![Cmd::Shell("make".into())];
        let deps: BTreeSet<String> = ["libcurl"].iter().map(|s| s.to_string()).collect();
        let dep_sigs: BTreeMap<String, String> =
            [("libcurl".to_string(), "dep-sig".to_string())].into();
        let sig_a = sig_of(&reg, &target, &commands, None, &dep_sigs, &deps);
        let sig_b = sig_of(&reg, &target, &commands, None, &dep_sigs, &deps);
        assert_eq!(sig_a, sig_b);
    }

    #[test]
    fn profile_actions_run_in_declared_order() {
        let reg = fixture();
        let target = bundled("curl", &["libcurl"], &[]);
        let def = component_def(&["step one", "step two"]);
        let mut target = target;
        target.prepare = vec!["prepare one".into(), "prepare two".into()];
        let commands =
            component_commands(&target, &def, &["--flag".to_string()], &node_ctx(), &reg).unwrap();

        let lines: Vec<String> = commands
            .iter()
            .map(|cmd| match cmd {
                Cmd::Shell(line) => line.clone(),
                Cmd::Program { .. } => panic!("组件动作不应出现 Program"),
            })
            .collect();
        assert_eq!(
            lines,
            vec!["prepare one", "prepare two", "step one", "step two"]
        );
    }

    #[test]
    fn profile_steps_change_updates_signature() {
        let reg = fixture();
        let target = bundled("curl", &["libcurl"], &[]);
        let ctx = node_ctx();

        let commands =
            component_commands(&target, &component_def(&["make"]), &[], &ctx, &reg).unwrap();
        let sig_a = sig_of(
            &reg,
            &target,
            &commands,
            None,
            &BTreeMap::new(),
            &BTreeSet::new(),
        );

        let commands = component_commands(
            &target,
            &component_def(&["make", "make check"]),
            &[],
            &ctx,
            &reg,
        )
        .unwrap();
        let sig_b = sig_of(
            &reg,
            &target,
            &commands,
            None,
            &BTreeMap::new(),
            &BTreeSet::new(),
        );

        assert_ne!(sig_a, sig_b);
        assert!(sig_a.contains("shell=make"));
        assert!(sig_b.contains("shell=make check"));
    }
}
