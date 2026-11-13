use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use crate::layout;
use crate::recipe::{Kind, Mode, Registry, Selection, Target};
use crate::state::ProjectState;

#[derive(Debug, Serialize)]
pub struct Sim {
    pub root: String,
    pub tree: String,
    pub order: Vec<Node>,
}

#[derive(Debug, Serialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    pub line: String,
    pub origin: String,
    pub mode: String,
    pub source: String,
    pub sha256: String,
    pub unpack: String,
    pub prefix: String,
    pub needs: Vec<String>,
    pub tools: Vec<String>,
    pub modules: Vec<String>,
    pub deps: Vec<DepNode>,
    pub features: Vec<String>,
    pub configure: Vec<String>,
    pub steps: Vec<String>,
    pub make: Vec<String>,
    pub artifacts: Vec<String>,
    pub ini: String,
    pub ini_lines: Vec<String>,
    pub shared_from: String,
}

#[derive(Debug, Serialize)]
pub struct DepNode {
    /// 依赖名；也是 `dep:NAME` 里的名字。
    pub name: String,
    pub features: Vec<String>,
}

pub(crate) struct Ctx<'a> {
    pub(crate) tree: &'a str,
    pub(crate) build: String,
    pub(crate) prefix: String,
}

pub fn build(
    reg: &Registry,
    order: &[&Target],
    root: &str,
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
) -> Result<Sim> {
    let php = reg.php()?;
    let tree = format!(".phprt/php-{}", php.version);
    let php_build = layout::build_dir(&php.name, Kind::Php, &php.url);
    let mut nodes = Vec::new();

    for t in order {
        let archive = layout::archive_name(&t.url).unwrap_or_else(|| "source".to_owned());
        let stem = layout::archive_stem(&archive);
        let mode = if t.kind == Kind::Ext {
            Some(sel.mode_of(t)?)
        } else {
            None
        };

        let prefix = match t.kind {
            Kind::Dep => format!("{tree}/deps/{}-{}", t.name, t.version),
            _ => tree.clone(),
        };
        let dir_name = match t.kind {
            Kind::Php => "php",
            _ => t.name.as_str(),
        };
        let build_dir = format!("build/{}/{}", dir_name, stem);
        let ctx = Ctx {
            tree: &tree,
            build: build_dir.clone(),
            prefix: prefix.clone(),
        };

        // ext 的 configure = 自己的参数 + 选中子配置的参数。
        let mut configure = if t.kind == Kind::Ext {
            ext_configure(reg, t, sel, &ctx)?
        } else {
            t.configure
                .iter()
                .map(|s| expand(s, &ctx, reg))
                .collect::<Result<Vec<_>>>()?
        };

        let mut needs: Vec<String> = t.needs.iter().map(|n| format!("dep:{n}")).collect();
        if t.kind == Kind::Ext {
            for need in sel.feature_needs(t) {
                let id = format!("dep:{need}");
                if !needs.contains(&id) {
                    needs.push(id);
                }
            }
        }
        let tools: Vec<String> = t.tools.iter().map(|n| format!("dep:{n}")).collect();
        let mut enabled_features: Vec<String> = Vec::new();

        if t.kind == Kind::Ext
            && let Some(selected) = sel.features_of(t)
        {
            enabled_features = selected.iter().cloned().collect();
        }

        if t.kind == Kind::Dep {
            if let Some(enabled) = features.get(&t.name) {
                enabled_features = enabled.clone();
                for name in enabled {
                    let Some(f) = t.features.get(name) else {
                        continue;
                    };
                    for need in &f.needs {
                        let id = format!("dep:{need}");
                        if !needs.contains(&id) {
                            needs.push(id);
                        }
                    }
                }
            }
            configure = dep_configure(reg, t, features.get(&t.name), &ctx)?;
        }

        // 每个内置扩展在 PHP configure 里只出现关闭参数或正向参数其中一组。
        if t.kind == Kind::Php {
            let php_ctx = Ctx {
                tree: &tree,
                build: build_dir.clone(),
                prefix: tree.clone(),
            };
            configure.extend(php_configure_flags(reg, t, sel, &php_ctx)?);
        }

        let make = t
            .make
            .iter()
            .map(|s| expand(s, &ctx, reg))
            .collect::<Result<Vec<_>>>()?;

        let mut source = t.url.clone();
        let mut unpack = match t.kind {
            Kind::Ext => String::new(),
            _ => build_dir.clone(),
        };
        let mut steps = Vec::new();
        let mut artifacts = t.artifacts.clone();
        let mut ini = String::new();
        let mut ini_lines = t.ini_lines.clone();

        match mode {
            Some(Mode::Shared) => {
                // builtin 从 PHP 源码树里拷，PECL 用自己下载的源码包。
                if t.origin == "builtin" {
                    source = format!("{php_build}/{}", t.shared_from);
                    unpack = format!("build/{}/php-{}", t.name, php.version);
                } else {
                    source = t.url.clone();
                    unpack = build_dir.clone();
                }
                let mut args = configure.clone();
                args.insert(0, format!("--with-php-config={tree}/bin/php-config"));
                steps = vec![
                    "phpize".to_owned(),
                    format!("./configure {}", args.join(" ")),
                    "make".to_owned(),
                    "make install".to_owned(),
                ];
                if artifacts.is_empty() {
                    artifacts = vec![format!("lib/php/extensions/*/{}.so", t.name)];
                }
                ini = t.ini.clone();
                if ini_lines.is_empty() {
                    ini_lines = vec![format!("extension={}.so", t.name)];
                }
                needs.push(format!("php:{}", php.version));
            }
            Some(Mode::Static) => {
                ini.clear();
                ini_lines.clear();
            }
            None => {}
        }

        let dep_nodes = if t.kind == Kind::Ext {
            sel.deps_of(t)
                .map(|(name, feats)| DepNode {
                    name: name.clone(),
                    features: feats.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };

        nodes.push(Node {
            id: t.id(),
            kind: t.kind.as_str().to_owned(),
            name: t.name.clone(),
            version: t.version.clone(),
            line: t.line.clone(),
            origin: if t.kind == Kind::Ext {
                t.origin.clone()
            } else {
                String::new()
            },
            mode: mode.map(|m| m.as_str().to_owned()).unwrap_or_default(),
            source,
            sha256: t.sha256.clone(),
            unpack,
            prefix,
            needs,
            tools,
            modules: t.modules.clone(),
            deps: dep_nodes,
            features: enabled_features,
            configure,
            steps,
            make,
            artifacts,
            ini,
            ini_lines,
            shared_from: t.shared_from.clone(),
        });
    }

    Ok(Sim {
        root: root.to_owned(),
        tree,
        order: nodes,
    })
}

/// dep 的 configure：基础参数 + 启用 feature 的参数 + 未启用 feature 的 disable 参数。
pub(crate) fn dep_configure(
    reg: &Registry,
    dep: &Target,
    enabled: Option<&Vec<String>>,
    ctx: &Ctx<'_>,
) -> Result<Vec<String>> {
    let mut out = dep
        .configure
        .iter()
        .map(|arg| expand(arg, ctx, reg))
        .collect::<Result<Vec<_>>>()?;
    let empty = Vec::new();
    let enabled = enabled.unwrap_or(&empty);
    for (name, feature) in &dep.features {
        if enabled.contains(name) {
            for arg in &feature.configure {
                out.push(expand(arg, ctx, reg)?);
            }
        } else {
            for arg in &feature.disable {
                out.push(expand(arg, ctx, reg)?);
            }
        }
    }
    Ok(out)
}

/// PHP configure 里的内置扩展开关：
/// `builtin_disable` 里的扩展没选 static 时用它的关闭参数，
/// 选了 static 时用 ext recipe 的 configure。
pub(crate) fn php_configure_flags(
    reg: &Registry,
    php: &Target,
    sel: &Selection,
    ctx: &Ctx<'_>,
) -> Result<Vec<String>> {
    let mut static_exts = BTreeSet::new();
    for name in &php.exts {
        let ext = reg.ext(name)?;
        if sel.mode_of(ext)? == Mode::Static {
            static_exts.insert(name.as_str());
        }
    }

    let mut out = Vec::new();
    for (name, flag) in &php.builtin_disable {
        if static_exts.contains(name.as_str()) {
            continue;
        }
        out.push(expand(flag, ctx, reg)?);
    }
    for name in &php.exts {
        let ext = reg.ext(name)?;
        if sel.mode_of(ext)? != Mode::Static {
            continue;
        }
        out.extend(ext_configure(reg, ext, sel, ctx)?);
    }
    Ok(out)
}

pub(crate) fn ext_configure(
    reg: &Registry,
    ext: &Target,
    sel: &Selection,
    ctx: &Ctx<'_>,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for arg in &ext.configure {
        out.push(expand(arg, ctx, reg)?);
    }
    if let Some(selected) = sel.features_of(ext) {
        for (name, def) in &ext.features {
            if !selected.contains(name) {
                continue;
            }
            for arg in &def.configure {
                out.push(expand(arg, ctx, reg)?);
            }
        }
    }
    for (dep, _) in sel.deps_of(ext) {
        let Some(flags) = ext.dep_configure.get(dep) else {
            continue;
        };
        for arg in flags {
            out.push(expand(arg, ctx, reg)?);
        }
    }
    Ok(out)
}

pub fn to_json(sim: &Sim) -> Result<String> {
    Ok(serde_json::to_string_pretty(sim)?)
}

pub fn to_text(sim: &Sim) -> String {
    let mut out = String::new();
    out.push_str("phprt plan\n");
    out.push_str(&format!("  root   {}\n", sim.root));
    out.push_str(&format!("  tree   {}\n", sim.tree));

    out.push_str("\nbuild order\n");
    for (i, n) in sim.order.iter().enumerate() {
        let extra = match n.kind.as_str() {
            "ext" => {
                let line = if n.line.is_empty() {
                    String::new()
                } else {
                    format!(", line {}", n.line)
                };
                format!("  ({} {}{})", n.origin, n.mode, line)
            }
            "php" => String::new(),
            _ => format!("  {}", n.version),
        };
        out.push_str(&format!("  {:>2}. {:<14}{}\n", i + 1, n.id, extra));
    }

    for (i, n) in sim.order.iter().enumerate() {
        let version = if n.kind == "php" {
            String::new()
        } else {
            format!(" {}", n.version)
        };
        out.push_str(&format!(
            "\n[{}/{}] {}{}\n",
            i + 1,
            sim.order.len(),
            n.id,
            version
        ));
        if !n.source.is_empty() {
            out.push_str(&format!("  source     {}\n", n.source));
        }
        if !n.sha256.is_empty() {
            out.push_str(&format!("  sha256     {}\n", n.sha256));
        }
        if !n.unpack.is_empty() {
            out.push_str(&format!("  unpack     {}\n", n.unpack));
        }
        out.push_str(&format!("  prefix     {}\n", n.prefix));
        if !n.line.is_empty() {
            out.push_str(&format!("  line       {}\n", n.line));
        }
        if !n.needs.is_empty() {
            out.push_str(&format!("  needs      {}\n", n.needs.join(", ")));
        }
        if !n.tools.is_empty() {
            out.push_str(&format!("  tools      {}\n", n.tools.join(", ")));
        }
        if !n.modules.is_empty() {
            out.push_str(&format!("  modules    {}\n", n.modules.join(", ")));
        }
        for dep in &n.deps {
            let feats = if dep.features.is_empty() {
                String::new()
            } else {
                format!(" [{}]", dep.features.join(", "))
            };
            out.push_str(&format!("  dep        {}{}\n", dep.name, feats));
        }
        if !n.features.is_empty() {
            out.push_str(&format!("  features   {}\n", n.features.join(", ")));
        }
        if n.mode == "static" {
            for arg in &n.configure {
                out.push_str(&format!("  flag       {}\n", arg));
            }
        } else if n.mode == "shared" {
            for step in &n.steps {
                out.push_str(&format!("  step       {}\n", step));
            }
        } else {
            for arg in &n.configure {
                out.push_str(&format!("  configure  {}\n", arg));
            }
            for step in &n.make {
                out.push_str(&format!("  make       {}\n", step));
            }
        }
        if !n.artifacts.is_empty() {
            out.push_str(&format!("  outputs    {}\n", n.artifacts.join(", ")));
        }
        if !n.ini.is_empty() {
            out.push_str(&format!("  ini        etc/conf.d/{}\n", n.ini));
            for line in &n.ini_lines {
                out.push_str(&format!("             {}\n", line));
            }
        }
    }
    out
}

pub fn graph_text(
    reg: &Registry,
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
) -> Result<String> {
    let mut out = String::new();
    let php = reg.php()?;
    for t in reg.targets.values() {
        match t.kind {
            Kind::Dep => {
                for dep in &t.needs {
                    out.push_str(&format!("{} -> dep:{}\n", t.id(), dep));
                }
                if let Some(enabled) = features.get(&t.name) {
                    for name in enabled {
                        if let Some(f) = t.features.get(name) {
                            for need in &f.needs {
                                out.push_str(&format!(
                                    "{} -> dep:{}  (feature {name})\n",
                                    t.id(),
                                    need
                                ));
                            }
                        }
                    }
                }
            }
            Kind::Ext => {
                for dep in &t.needs {
                    out.push_str(&format!("{} -> dep:{}\n", t.id(), dep));
                }
                for tool in &t.tools {
                    out.push_str(&format!("{} -> dep:{}  (tool)\n", t.id(), tool));
                }
                if sel.mode_of(t)? == Mode::Shared {
                    out.push_str(&format!("{} -> php:{}  (shared)\n", t.id(), php.version));
                }
            }
            Kind::Php => {
                for name in &t.exts {
                    let ext = reg.ext(name)?;
                    if sel.mode_of(ext)? == Mode::Static {
                        out.push_str(&format!("{} -> ext:{}\n", t.id(), name));
                    }
                }
            }
        }
    }
    Ok(out)
}

pub fn ext_list_text(
    reg: &Registry,
    state: &ProjectState,
    installed_only: bool,
    pending_only: bool,
) -> String {
    let mut out = format!(
        "{:<12} {:<9} {:<15} {:<10} {}\n",
        "name", "origin", "link", "version", "status"
    );
    for ext in reg.of_kind(Kind::Ext) {
        let status = state.ext_status(&ext.name);
        if installed_only && status != "installed" {
            continue;
        }
        if pending_only && status != "pending" {
            continue;
        }
        out.push_str(&format!(
            "{:<12} {:<9} {:<15} {:<10} {}\n",
            ext.name,
            ext.origin,
            ext.link.join(","),
            ext.version,
            status
        ));
    }
    out
}

pub fn list_text(reg: &Registry) -> String {
    let mut out = String::new();
    for t in reg.targets.values() {
        let mut notes = Vec::new();
        if !t.line.is_empty() {
            notes.push(format!("line={}", t.line));
        }
        if !t.needs.is_empty() {
            notes.push(format!("needs={}", t.needs.join(",")));
        }
        if !t.tools.is_empty() {
            notes.push(format!("tools={}", t.tools.join(",")));
        }
        if !t.exts.is_empty() {
            notes.push(format!("exts={}", t.exts.join(",")));
        }
        if !t.modules.is_empty() {
            notes.push(format!("modules={}", t.modules.join(",")));
        }
        if t.kind == Kind::Ext {
            notes.push(format!("origin={}", t.origin));
            notes.push(format!("link={}", t.link.join(",")));
            if !t.dep_configure.is_empty() {
                notes.push(format!(
                    "dep_configure={}",
                    t.dep_configure
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
        }
        if !t.features.is_empty() {
            notes.push(format!(
                "features={}",
                t.features.keys().cloned().collect::<Vec<_>>().join(",")
            ));
        }
        out.push_str(&format!(
            "{:<16} {:<10} {}\n",
            t.id(),
            t.version,
            notes.join(" ")
        ));
    }
    out
}

pub(crate) fn expand(input: &str, ctx: &Ctx<'_>, reg: &Registry) -> Result<String> {
    let mut out = String::new();
    let mut rest = input;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            break;
        };
        out.push_str(&rest[..start]);
        let key = &after[..end];
        let value = match key {
            "tree" => ctx.tree.to_owned(),
            "deps" => format!("{}/deps", ctx.tree),
            "build" => ctx.build.clone(),
            "prefix" => ctx.prefix.clone(),
            _ if key.starts_with("dep:") => {
                let name = &key[4..];
                let dep = reg
                    .get(&format!("dep:{name}"))
                    .ok_or_else(|| anyhow!("占位符 {{dep:{name}}} 找不到依赖"))?;
                format!("{}/deps/{}-{}", ctx.tree, dep.name, dep.version)
            }
            _ => bail!("未知占位符 {{{key}}}（在 {input:?} 里）"),
        };
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::Feature;

    fn libcurl() -> Target {
        let mut target = Target {
            kind: Kind::Dep,
            name: "libcurl".into(),
            version: "8.23.0".into(),
            url: "https://example.com/curl.tar.xz".into(),
            configure: vec!["./configure".into(), "--prefix={prefix}".into()],
            ..blank()
        };
        target.features.insert(
            "ssl".into(),
            Feature {
                needs: vec!["openssl".into()],
                configure: vec!["--with-openssl={dep:openssl}".into()],
                disable: vec!["--without-ssl".into()],
                requires: Default::default(),
            },
        );
        target
    }

    fn blank() -> Target {
        Target {
            kind: Kind::Dep,
            name: String::new(),
            version: String::new(),
            url: String::new(),
            sha256: String::new(),
            needs: vec![],
            tools: vec![],
            builtin_exts: vec![],
            builtin_disable: Default::default(),
            exts: vec![],
            modules: vec![],
            origin: "builtin".into(),
            link: vec![],
            dep_configure: Default::default(),
            shared_from: String::new(),
            ini: String::new(),
            ini_lines: vec![],
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

    fn openssl() -> Target {
        Target {
            kind: Kind::Dep,
            name: "openssl".into(),
            version: "3.5.9".into(),
            url: "https://example.com/openssl.tar.gz".into(),
            ..blank()
        }
    }

    fn openssl_ext() -> Target {
        Target {
            kind: Kind::Ext,
            name: "openssl".into(),
            version: "8.5.12".into(),
            modules: vec!["openssl".into()],
            origin: "builtin".into(),
            link: vec!["static".into(), "shared".into()],
            needs: vec!["openssl".into()],
            configure: vec!["--with-openssl={dep:openssl}".into()],
            ..blank()
        }
    }

    fn php(exts: &[&str]) -> Target {
        Target {
            kind: Kind::Php,
            name: "8.5.12".into(),
            version: "8.5.12".into(),
            url: "https://example.com/php.tar.xz".into(),
            builtin_exts: vec!["openssl".into()],
            builtin_disable: BTreeMap::from([(
                "openssl".to_owned(),
                "--without-openssl".to_owned(),
            )]),
            exts: exts.iter().map(|s| s.to_string()).collect(),
            ..blank()
        }
    }

    #[test]
    fn php_gets_one_side_of_the_static_flags() {
        let reg = Registry::from_targets(vec![openssl(), openssl_ext(), php(&[])]).unwrap();
        let php_target = reg.php().unwrap();
        let ctx = Ctx {
            tree: "/t",
            build: "/b".into(),
            prefix: "/t".into(),
        };
        let sel = Selection::default();
        let args = php_configure_flags(&reg, php_target, &sel, &ctx).unwrap();
        assert!(args.contains(&"--without-openssl".to_owned()));
        assert!(!args.iter().any(|arg| arg.starts_with("--with-openssl")));

        let reg =
            Registry::from_targets(vec![openssl(), openssl_ext(), php(&["openssl"])]).unwrap();
        let php_target = reg.php().unwrap();
        let sel = Selection::default();
        let args = php_configure_flags(&reg, php_target, &sel, &ctx).unwrap();
        assert!(args.contains(&"--with-openssl=/t/deps/openssl-3.5.9".to_owned()));
        assert!(!args.contains(&"--without-openssl".to_owned()));
    }

    #[test]
    fn disabled_feature_adds_its_disable_args() {
        let reg = Registry::from_targets(vec![libcurl(), openssl()]).unwrap();
        let dep = reg.get("dep:libcurl").unwrap();
        let ctx = Ctx {
            tree: "/t",
            build: "/b".into(),
            prefix: "/p".into(),
        };

        let args = dep_configure(&reg, dep, None, &ctx).unwrap();
        assert!(args.contains(&"--without-ssl".to_owned()));

        let enabled = vec!["ssl".to_owned()];
        let args = dep_configure(&reg, dep, Some(&enabled), &ctx).unwrap();
        assert!(args.contains(&"--with-openssl=/t/deps/openssl-3.5.9".to_owned()));
        assert!(!args.contains(&"--without-ssl".to_owned()));
    }
}
