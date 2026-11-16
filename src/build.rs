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
use crate::recipe::{Kind, Registry, Selection, Strategy, Target};
use crate::render::{self, Ctx};

pub struct Options<'a> {
    pub root: &'a Path,
    pub recipes: &'a Path,
    pub ui: Mode,
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
    let tree = format!(
        "{}/.phprt/{}-{}",
        root.display(),
        product.name,
        product.version
    );
    let tree_path = PathBuf::from(&tree);
    let product_build = root.join(layout::build_dir_of(product));
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let vendor = fs::canonicalize(opts.recipes.join("../vendor/perl")).ok();

    let mut steps = Vec::new();
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
                let mut commands = Vec::new();
                for line in &target.prepare {
                    commands.push(Cmd::Shell(render::expand(line, &node_ctx, reg)?));
                }
                let flags = render::component_configure(reg, target, sel, &node_ctx)?;
                for template in &def.steps {
                    let line = template
                        .replace("{name}", &target.name)
                        .replace("{product}", &tree)
                        .replace("{flags}", &flags.join(" "))
                        .replace("{build}", &work.display().to_string())
                        .replace("{prefix}", &tree);
                    commands.push(Cmd::Shell(render::expand(&line, &node_ctx, reg)?));
                }
                let artifacts = if target.artifacts.is_empty() {
                    artifacts_expanded(&def.artifacts, target)
                } else {
                    artifacts_expanded(&target.artifacts, target)
                };
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
        let sig = signature(target, strategy, &commands, &env, &artifacts);

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
    actions::run(reg, product, sel, &tree)?;
    verify(reg, product, &tree)?;
    let target = format!("{}@{}", product.name, product.series);
    write_entry(&root, product, opts.recipes, &target)?;
    Ok(())
}

/// `remove` 之后立刻停用：删掉这个组件的登记文件。树不存在时什么也不做。
pub fn disable_component(
    root: &Path,
    product: &Target,
    component: &Target,
    mode: &str,
) -> Result<()> {
    let Some(register) = register_action(product, mode) else {
        return Ok(());
    };
    let Some(dir) = product.profile.register_dir(register) else {
        return Ok(());
    };
    if component.ini.is_empty() {
        return Ok(());
    }
    let path = root
        .join(format!(".phprt/{}-{}", product.name, product.version))
        .join(dir)
        .join(&component.ini);
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("删不了 {}", path.display()))?;
    }
    Ok(())
}

fn ctx<'a>(tree: &'a str, build: &Path, prefix: &Path) -> Ctx<'a> {
    Ctx {
        tree,
        build: build.display().to_string(),
        prefix: prefix.display().to_string(),
    }
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

fn register_action<'a>(product: &'a Target, mode: &str) -> Option<&'a str> {
    product.profile.register.get(mode).map(String::as_str)
}

fn signature(
    target: &Target,
    strategy: Option<Strategy>,
    commands: &[Cmd],
    env: &BTreeMap<String, String>,
    artifacts: &[PathBuf],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("id={}\n", target.id()));
    out.push_str(&format!("version={}\n", target.version));
    out.push_str(&format!(
        "strategy={}\n",
        strategy.map(|s| s.as_str()).unwrap_or("")
    ));
    for cmd in commands {
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
    for (key, value) in env {
        out.push_str(&format!("env={key}={value}\n"));
    }
    for artifact in artifacts {
        out.push_str(&format!("artifact={}\n", artifact.display()));
    }
    out
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
            .join("build")
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
    if let Some(existing) = std::env::var_os("PATH") {
        path.push(existing.to_string_lossy().into_owned());
    }
    if let Some(existing) = std::env::var_os("PKG_CONFIG_PATH") {
        pc.push(existing.to_string_lossy().into_owned());
    }
    if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
        lib_path.push(existing.to_string_lossy().into_owned());
    }

    env.insert("CPPFLAGS".to_owned(), join_env("CPPFLAGS", &cpp));
    env.insert("LDFLAGS".to_owned(), join_env("LDFLAGS", &ld));
    env.insert("PKG_CONFIG_PATH".to_owned(), pc.join(":"));
    env.insert("LD_LIBRARY_PATH".to_owned(), lib_path.join(":"));
    env.insert("PATH".to_owned(), path.join(":"));
    env.insert("CFLAGS".to_owned(), append_env("CFLAGS", "-O2"));
    env.insert("CXXFLAGS".to_owned(), append_env("CXXFLAGS", "-O2"));
    env.insert("MAKEFLAGS".to_owned(), format!("-j{jobs}"));
    if let Some(vendor) = vendor {
        let mut perl = vendor.display().to_string();
        if let Some(existing) = std::env::var_os("PERL5LIB")
            && !existing.is_empty()
        {
            perl.push(':');
            perl.push_str(&existing.to_string_lossy());
        }
        env.insert("PERL5LIB".to_owned(), perl);
    }
    env
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

fn join_env(key: &str, base: &str) -> String {
    let base = base.trim_end();
    match std::env::var(key) {
        Ok(existing) if !existing.is_empty() => format!("{base} {existing}"),
        _ => base.to_owned(),
    }
}

fn append_env(key: &str, base: &str) -> String {
    match std::env::var(key) {
        Ok(existing) if !existing.is_empty() => format!("{base} {existing}"),
        _ => base.to_owned(),
    }
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
    if product.profile.entry.is_empty() {
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
    let Ok(entries) = fs::read_dir(root.join(".phprt")) else {
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
}
