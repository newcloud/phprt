use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use phprt_build::{Cmd, Ini, Step};
use phprt_ui::Mode;

use crate::layout;
use crate::recipe::{Kind, Mode as LinkMode, Registry, Selection, Target};
use crate::render::{self, Ctx};

const MYPHP: &str = include_str!("../templates/myphp.tmpl");

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
    need_commands()?;

    let root = fs::canonicalize(opts.root)
        .with_context(|| format!("读不到项目根 {}", opts.root.display()))?;
    let php = reg.php()?;
    let tree = format!("{}/.phprt/php-{}", root.display(), php.version);
    let tree_path = PathBuf::from(&tree);
    let php_build = build_dir(&root, php);
    let tree_abs = tree_path.clone();
    let jobs = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let vendor = fs::canonicalize(opts.recipes.join("../vendor/perl")).ok();
    let tool_names: BTreeSet<String> = reg
        .of_kind(Kind::Ext)
        .flat_map(|ext| ext.tools.iter().cloned())
        .collect();

    let autoconf = order
        .iter()
        .find(|t| t.kind == Kind::Dep && t.name == "autoconf")
        .map(|t| dep_prefix(&tree_path, t).join("bin"));

    let mut steps = Vec::new();
    for target in order {
        let mode = if target.kind == Kind::Ext {
            Some(sel.mode_of(target)?)
        } else {
            None
        };
        if target.kind == Kind::Ext && mode == Some(LinkMode::Static) {
            continue;
        }

        let prefix = match target.kind {
            Kind::Dep => dep_prefix(&tree_path, target),
            _ => tree_abs.clone(),
        };
        let work = work_dir(&root, target, php, mode);
        let deps = link_closure(reg, sel, features, target)?;
        let link_prefixes: Vec<PathBuf> = deps
            .iter()
            .filter(|name| !tool_names.contains(*name))
            .filter_map(|name| reg.get(&format!("dep:{name}")))
            .map(|dep| dep_prefix(&tree_path, dep))
            .collect();
        let tool_prefixes: Vec<PathBuf> = target
            .tools
            .iter()
            .filter_map(|name| reg.get(&format!("dep:{name}")))
            .map(|dep| dep_prefix(&tree_path, dep))
            .collect();

        let env = build_env(
            &tree,
            &link_prefixes,
            &tool_prefixes,
            jobs,
            vendor.as_deref(),
            autoconf.as_deref(),
        );

        let (commands, artifacts) = match target.kind {
            Kind::Dep => {
                let args = render::dep_configure(
                    reg,
                    target,
                    features.get(&target.name),
                    &ctx(&tree, &work, &prefix),
                )?;
                (
                    dep_commands(args, &target.make, &ctx(&tree, &work, &prefix), reg)?,
                    &target.artifacts,
                )
            }
            Kind::Php => {
                let mut args = expand_all(&target.configure, &ctx(&tree, &work, &prefix), reg)?;
                let ext_ctx = ctx(&tree, &php_build, &tree_path);
                args.extend(render::php_configure_flags(reg, target, sel, &ext_ctx)?);
                let mut commands = Vec::new();
                if !args.is_empty() {
                    commands.push(Cmd::Program {
                        program: "./configure".to_owned(),
                        args,
                    });
                }
                for line in &target.make {
                    commands.push(Cmd::Shell(render::expand(
                        line,
                        &ctx(&tree, &work, &prefix),
                        reg,
                    )?));
                }
                (commands, &target.artifacts)
            }
            Kind::Ext => {
                let ext_ctx = ctx(&tree, &work, &prefix);
                let mut args = render::ext_configure(reg, target, sel, &ext_ctx)?;
                args.insert(0, format!("--with-php-config={}/bin/php-config", tree));
                let commands = vec![
                    Cmd::Program {
                        program: "phpize".to_owned(),
                        args: Vec::new(),
                    },
                    Cmd::Program {
                        program: "./configure".to_owned(),
                        args,
                    },
                    Cmd::Shell("make".to_owned()),
                    Cmd::Shell("make install".to_owned()),
                ];
                (commands, &target.artifacts)
            }
        };

        let artifacts: Vec<PathBuf> = if artifacts.is_empty() && target.kind == Kind::Ext {
            vec![PathBuf::from(format!(
                "lib/php/extensions/*/{}.so",
                target.name
            ))]
        } else {
            artifacts.iter().map(PathBuf::from).collect()
        }
        .into_iter()
        .map(|rel| prefix.join(rel))
        .collect();

        let ini = match (target.kind, mode) {
            (Kind::Ext, Some(LinkMode::Shared)) => Some(Ini {
                path: PathBuf::from(&tree).join("etc/conf.d").join(&target.ini),
                lines: if target.ini_lines.is_empty() {
                    vec![format!("extension={}.so", target.name)]
                } else {
                    target.ini_lines.clone()
                },
            }),
            _ => None,
        };

        let copy_from = match (target.kind, mode, target.origin.as_str()) {
            (Kind::Ext, Some(LinkMode::Shared), "builtin") => {
                Some(php_build.join(&target.shared_from))
            }
            _ => None,
        };
        let sig = signature(target, mode, &commands, &env, &artifacts);

        steps.push(Step {
            tag: format!(
                "[{}] {}{}",
                match target.kind {
                    Kind::Php => "php",
                    Kind::Dep => "deps",
                    Kind::Ext => "ext",
                },
                target.name,
                if target.kind == Kind::Php {
                    String::new()
                } else {
                    format!(" {}", target.version)
                }
            ),
            cwd: work.clone(),
            copy_from,
            commands,
            env,
            artifacts,
            ini,
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
    deploy_libs(order, &tree)?;
    write_tree_config(reg, sel, &tree)?;
    verify(reg, sel, &tree)?;
    write_entry(&root, &tree, &php.version)?;
    Ok(())
}

/// `ext remove` 之后立刻停用：删掉这个扩展的 ini。树不存在时什么也不做。
pub fn disable_ext(root: &Path, php_version: &str, ext: &Target) -> Result<()> {
    if ext.ini.is_empty() {
        return Ok(());
    }
    let path = root
        .join(format!(".phprt/php-{php_version}"))
        .join("etc/conf.d")
        .join(&ext.ini);
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

fn expand_all(args: &[String], ctx: &Ctx<'_>, reg: &Registry) -> Result<Vec<String>> {
    args.iter()
        .map(|arg| render::expand(arg, ctx, reg))
        .collect()
}

fn dep_commands(
    mut configure: Vec<String>,
    make: &[String],
    ctx: &Ctx<'_>,
    reg: &Registry,
) -> Result<Vec<Cmd>> {
    let mut commands = Vec::new();
    if !configure.is_empty() {
        let program = if configure[0].starts_with("./") {
            configure.remove(0)
        } else {
            "./configure".to_owned()
        };
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

fn signature(
    target: &Target,
    mode: Option<LinkMode>,
    commands: &[Cmd],
    env: &BTreeMap<String, String>,
    artifacts: &[PathBuf],
) -> String {
    let mut out = String::new();
    out.push_str(&format!("id={}\n", target.id()));
    out.push_str(&format!("version={}\n", target.version));
    out.push_str(&format!(
        "mode={}\n",
        mode.map(|m| m.as_str()).unwrap_or("")
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

fn build_dir(root: &Path, target: &Target) -> PathBuf {
    let dir = layout::build_dir(&target.name, target.kind, &target.url);
    root.join(dir)
}

fn work_dir(root: &Path, target: &Target, php: &Target, mode: Option<LinkMode>) -> PathBuf {
    match (target.kind, mode, target.origin.as_str()) {
        (Kind::Ext, Some(LinkMode::Shared), "builtin") => root
            .join("build")
            .join(&target.name)
            .join(format!("php-{}", php.version)),
        _ => build_dir(root, target),
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
        Kind::Ext => {
            for need in &target.needs {
                walk_dep(reg, features, need, &mut out);
            }
            for need in sel.feature_needs(target) {
                walk_dep(reg, features, need, &mut out);
            }
        }
        Kind::Php => {
            for name in &target.exts {
                let ext = reg.ext(name)?;
                if sel.mode_of(ext)? != LinkMode::Static {
                    continue;
                }
                for need in &ext.needs {
                    walk_dep(reg, features, need, &mut out);
                }
                for need in sel.feature_needs(ext) {
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
    deps: &[PathBuf],
    tools: &[PathBuf],
    jobs: usize,
    vendor: Option<&Path>,
    autoconf: Option<&Path>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    let mut cpp = String::new();
    let mut ld = String::new();
    let mut pc: Vec<String> = Vec::new();
    let mut lib_path: Vec<String> = vec![format!("{tree}/lib/phprt")];
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
    if let Some(bin) = autoconf {
        env.insert(
            "PHP_AUTOCONF".to_owned(),
            bin.join("autoconf").display().to_string(),
        );
        env.insert(
            "PHP_AUTOHEADER".to_owned(),
            bin.join("autoheader").display().to_string(),
        );
    }
    env
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

fn deploy_libs(order: &[&Target], tree: &Path) -> Result<()> {
    let flat = tree.join("lib/phprt");
    if flat.is_dir() {
        fs::remove_dir_all(&flat).with_context(|| format!("清不了 {}", flat.display()))?;
    }
    fs::create_dir_all(&flat).with_context(|| format!("建不了 {}", flat.display()))?;

    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    for target in order.iter().filter(|t| t.kind == Kind::Dep) {
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

    if let Ok(entries) = fs::read_dir(tree.join("bin")) {
        for entry in entries {
            let path = entry?.path();
            if !is_elf(&path) {
                continue;
            }
            patchelf_rpath(&path, "$ORIGIN/../lib/phprt")?;
        }
    }
    let ext_root = tree.join("lib/php/extensions");
    if let Ok(dirs) = fs::read_dir(&ext_root) {
        for dir in dirs {
            let dir = dir?.path();
            if !dir.is_dir() {
                continue;
            }
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries {
                    let path = entry?.path();
                    if is_elf(&path) {
                        patchelf_rpath(&path, "$ORIGIN/../../../phprt")?;
                    }
                }
            }
        }
    }
    Ok(())
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

fn write_tree_config(reg: &Registry, sel: &Selection, tree: &Path) -> Result<()> {
    let etc = tree.join("etc");
    let conf = etc.join("conf.d");
    fs::create_dir_all(&conf).with_context(|| format!("建不了 {}", conf.display()))?;

    let ini = etc.join("php.ini");
    if !ini.exists() {
        fs::write(
            &ini,
            "[PHP]\ndate.timezone = Asia/Shanghai\nmemory_limit = 256M\nerror_reporting = E_ALL\ndisplay_errors = On\nlog_errors = On\n",
        )
        .with_context(|| format!("写不了 {}", ini.display()))?;
    }

    let selected: BTreeSet<&str> = reg.php()?.exts.iter().map(String::as_str).collect();
    for ext in reg.of_kind(Kind::Ext) {
        if ext.ini.is_empty() {
            continue;
        }
        let enabled = selected.contains(ext.name.as_str()) && sel.mode_of(ext)? == LinkMode::Shared;
        if !enabled {
            let path = conf.join(&ext.ini);
            if path.exists() {
                fs::remove_file(&path)
                    .with_context(|| format!("删不了旧 ini {}", path.display()))?;
            }
        }
    }

    let ssl_dir = etc.join("ssl");
    fs::create_dir_all(&ssl_dir).with_context(|| format!("建不了 {}", ssl_dir.display()))?;
    let cert = ssl_dir.join("cert.pem");
    if !cert.is_file() {
        let candidates = [
            "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/pki/tls/certs/ca-bundle.crt",
            "/etc/ssl/cert.pem",
        ];
        if let Some(source) = candidates.iter().find(|path| Path::new(path).is_file()) {
            fs::copy(source, &cert).with_context(|| format!("复制不了 CA {}", source))?;
        } else {
            eprintln!("注意: 没找到系统 CA，HTTPS 校验会失败");
        }
    }
    fs::write(
        conf.join("00-ssl.ini"),
        "; 由 phprt 生成\nopenssl.cafile = \"${SSL_CERT_FILE}\"\ncurl.cainfo    = \"${SSL_CERT_FILE}\"\n",
    )
    .with_context(|| format!("写不了 {}", conf.join("00-ssl.ini").display()))?;
    Ok(())
}

fn verify(reg: &Registry, sel: &Selection, tree: &Path) -> Result<()> {
    let php = tree.join("bin/php");
    let version = Command::new(&php)
        .arg("-v")
        .output()
        .with_context(|| format!("跑不起来 {}", php.display()))?;
    if !version.status.success() {
        bail!(
            "{} -v 失败:\n{}",
            php.display(),
            String::from_utf8_lossy(&version.stderr)
        );
    }

    let output = Command::new(&php)
        .arg("-m")
        .output()
        .with_context(|| format!("跑不起来 {} -m", php.display()))?;
    if !output.status.success() {
        bail!(
            "{} -m 失败:\n{}",
            php.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let loaded = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let mut missing = Vec::new();
    for name in &reg.php()?.exts {
        let ext = reg.ext(name)?;
        if sel.mode_of(ext)? != LinkMode::Static && sel.mode_of(ext)? != LinkMode::Shared {
            continue;
        }
        for module in &ext.modules {
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
            "PHP 启动后缺少扩展:\n  {}\n{}",
            missing.join("\n  "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn write_entry(root: &Path, tree: &Path, version: &str) -> Result<()> {
    let myphp = root.join("myphp");
    fs::write(&myphp, MYPHP).with_context(|| format!("写不了 {}", myphp.display()))?;
    let mut perms = fs::metadata(&myphp)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&myphp, perms)?;

    let current = root.join(".phprt/current");
    if current.exists() || fs::symlink_metadata(&current).is_ok() {
        fs::remove_file(&current).with_context(|| format!("删不了 {}", current.display()))?;
    }
    std::os::unix::fs::symlink(format!("php-{version}"), &current)
        .with_context(|| format!("建不了 {}", current.display()))?;
    let _ = tree;
    Ok(())
}

fn need_commands() -> Result<()> {
    let tools = [
        "gcc",
        "g++",
        "make",
        "ar",
        "ranlib",
        "pkg-config",
        "patchelf",
        "m4",
        "perl",
        "sh",
    ];
    let missing: Vec<&str> = tools
        .iter()
        .copied()
        .filter(|tool| !on_path(tool))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    bail!(
        "缺少构建工具: {}（Fedora: sudo dnf install gcc gcc-c++ make binutils pkgconf-pkg-config patchelf tar m4 perl）",
        missing.join(", ")
    )
}

fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        candidate.is_file()
            && candidate
                .metadata()
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    })
}
