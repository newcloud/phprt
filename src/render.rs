use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

use crate::layout;
use crate::recipe::{Kind, Registry, Selection, Strategy, Target};
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
    pub source: String,
    pub mode: String,
    pub strategy: String,
    pub url: String,
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
    pub ini_dir: String,
    pub ini_lines: Vec<String>,
    pub source_path: String,
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
    let product = reg.product_any()?;
    let tree = format!(".phprt/{}-{}", product.name, product.version);
    let mut nodes = Vec::new();

    for t in order {
        let archive = layout::archive_name(&t.url).unwrap_or_else(|| "source".to_owned());
        let stem = layout::archive_stem(&archive);
        let mode = if t.kind == Kind::Component {
            Some(sel.mode_of(t))
        } else {
            None
        };
        let strategy = match (&mode, t.kind) {
            (Some(mode), Kind::Component) => Some(reg.product_mode(t, mode)?.strategy_kind()?),
            _ => None,
        };

        let prefix = match t.kind {
            Kind::Dep => format!("{tree}/deps/{}-{}", t.name, t.version),
            _ => tree.clone(),
        };
        let build_dir = format!("build/{}/{}", t.name, stem);
        let ctx = Ctx {
            tree: &tree,
            build: build_dir.clone(),
            prefix: prefix.clone(),
        };

        let mut configure = match t.kind {
            Kind::Component if strategy == Some(Strategy::Fold) => {
                let mut out = component_configure(reg, t, sel, &ctx)?;
                if let Some(mode) = mode.as_deref() {
                    out.extend(fold_templates(reg, product, t, mode, &ctx)?);
                }
                out
            }
            Kind::Component => component_configure(reg, t, sel, &ctx)?,
            Kind::Product => t
                .configure
                .iter()
                .map(|s| expand(s, &ctx, reg))
                .collect::<Result<Vec<_>>>()?,
            Kind::Dep => t
                .configure
                .iter()
                .map(|s| expand(s, &ctx, reg))
                .collect::<Result<Vec<_>>>()?,
        };

        let mut needs: Vec<String> = t.needs.iter().map(|n| format!("dep:{n}")).collect();
        if t.kind == Kind::Component {
            for need in sel.feature_needs(t) {
                let id = format!("dep:{need}");
                if !needs.contains(&id) {
                    needs.push(id);
                }
            }
        }
        let tools: Vec<String> = t.tools.iter().map(|n| format!("dep:{n}")).collect();
        let mut enabled_features: Vec<String> = Vec::new();

        if t.kind == Kind::Component
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

        // 产品的 configure 追加 fold 组件的参数和内置组件的关闭参数。
        if t.kind == Kind::Product {
            let product_ctx = Ctx {
                tree: &tree,
                build: build_dir.clone(),
                prefix: tree.clone(),
            };
            configure.extend(product_configure_flags(reg, t, sel, &product_ctx)?);
        }

        let make = t
            .make
            .iter()
            .map(|s| expand(s, &ctx, reg))
            .collect::<Result<Vec<_>>>()?;

        let mut unpack = match t.kind {
            Kind::Component => String::new(),
            _ => build_dir.clone(),
        };
        let mut steps = Vec::new();
        let mut artifacts = t.artifacts.clone();
        let mut ini = String::new();
        let mut ini_dir = String::new();
        let mut ini_lines = t.ini_lines.clone();

        match (t.kind, strategy) {
            (Kind::Component, Some(Strategy::Standalone)) => {
                let mode_name = mode.clone().unwrap_or_default();
                let def = reg.product_mode(t, &mode_name)?;
                let flags = configure.join(" ");
                unpack = if t.source == "bundled" {
                    format!("build/{}/{}-{}", t.name, product.name, product.version)
                } else {
                    build_dir.clone()
                };
                steps = def
                    .steps
                    .iter()
                    .map(|step| {
                        let step = step
                            .replace("{name}", &t.name)
                            .replace("{product}", ctx.tree)
                            .replace("{product_version}", &product.version)
                            .replace("{flags}", &flags)
                            .replace("{build}", &ctx.build)
                            .replace("{prefix}", &ctx.prefix);
                        expand(&step, &ctx, reg)
                    })
                    .collect::<Result<Vec<_>>>()?;
                if artifacts.is_empty() {
                    artifacts = def
                        .artifacts
                        .iter()
                        .map(|pattern| pattern.replace("{name}", &t.name))
                        .collect();
                }
                if let Some(dir) = register_action(product, &mode_name)
                    .and_then(|register| product.profile.register_dir(register))
                    && !t.ini.is_empty()
                {
                    ini = t.ini.clone();
                    ini_dir = dir.to_owned();
                    if ini_lines.is_empty() {
                        ini_lines = vec![format!("extension={}.so", t.name)];
                    }
                }
                needs.push(product.id());
            }
            (Kind::Component, Some(Strategy::Fold)) => {
                ini.clear();
                ini_lines.clear();
                if !t.ini.is_empty() {
                    ini.clear();
                }
            }
            (Kind::Product, _) => {
                for name in &t.components {
                    let component = reg.component(name)?;
                    let mode_name = sel.mode_of(component);
                    if reg.product_mode(component, &mode_name)?.strategy_kind()? == Strategy::Fold {
                        artifacts.extend(
                            reg.product_mode(component, &mode_name)?
                                .artifacts
                                .iter()
                                .map(|pattern| pattern.replace("{name}", &component.name)),
                        );
                    }
                }
            }
            _ => {}
        }

        let dep_nodes = if t.kind == Kind::Component {
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
            source: if t.kind == Kind::Component {
                t.source.clone()
            } else {
                String::new()
            },
            mode: mode.unwrap_or_default(),
            strategy: strategy.map(|s| s.as_str()).unwrap_or_default().to_owned(),
            url: t.url.clone(),
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
            ini_dir,
            ini_lines,
            source_path: t.source_path.clone(),
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

/// 产品 configure 里由组件贡献的参数：
/// `bundled_disable` 里的组件没选 fold 时用它的关闭参数，
/// 选了 fold 时用 profile 的模板和组件自己的 configure。
pub(crate) fn product_configure_flags(
    reg: &Registry,
    product: &Target,
    sel: &Selection,
    ctx: &Ctx<'_>,
) -> Result<Vec<String>> {
    let mut folded = BTreeSet::new();
    for name in &product.components {
        let component = reg.component(name)?;
        let mode = sel.mode_of(component);
        if reg.product_mode(component, &mode)?.strategy_kind()? == Strategy::Fold {
            folded.insert(name.as_str());
        }
    }

    let mut out = Vec::new();
    for (name, flag) in &product.bundled_disable {
        if folded.contains(name.as_str()) {
            continue;
        }
        out.push(expand(flag, ctx, reg)?);
    }
    for name in &product.components {
        let component = reg.component(name)?;
        if !folded.contains(name.as_str()) {
            continue;
        }
        let mode = sel.mode_of(component);
        out.extend(component_configure(reg, component, sel, ctx)?);
        out.extend(fold_templates(reg, product, component, &mode, ctx)?);
    }
    Ok(out)
}

/// profile 为一个 fold 模式声明的参数模板。
fn fold_templates(
    reg: &Registry,
    product: &Target,
    component: &Target,
    mode: &str,
    ctx: &Ctx<'_>,
) -> Result<Vec<String>> {
    let def = reg.product_mode(component, mode)?;
    let source = absolute_source(ctx, &component_source(reg, product, component)?);
    def.configure
        .iter()
        .map(|template| {
            let replaced = template.replace("{component_source}", &source);
            expand(&replaced, ctx, reg)
        })
        .collect()
}

pub(crate) fn component_configure(
    reg: &Registry,
    component: &Target,
    sel: &Selection,
    ctx: &Ctx<'_>,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for arg in &component.configure {
        out.push(expand(arg, ctx, reg)?);
    }
    if let Some(selected) = sel.features_of(component) {
        for (name, def) in &component.features {
            if !selected.contains(name) {
                continue;
            }
            for arg in &def.configure {
                out.push(expand(arg, ctx, reg)?);
            }
        }
    }
    for (dep, _) in sel.deps_of(component) {
        let Some(flags) = component.dep_configure.get(dep) else {
            continue;
        };
        for arg in flags {
            out.push(expand(arg, ctx, reg)?);
        }
    }
    Ok(out)
}

fn register_action<'a>(product: &'a Target, mode: &str) -> Option<&'a str> {
    product.profile.register.get(mode).map(String::as_str)
}

/// bundled 组件的源码位于产品源码树里；external 组件和 dep 用解压目录。
fn component_source(_reg: &Registry, product: &Target, component: &Target) -> Result<String> {
    match component.kind {
        Kind::Component if component.source == "bundled" => Ok(format!(
            "{}/{}",
            layout::build_dir_of(product),
            bundled_source_path(product, component)?
        )),
        _ => Ok(layout::build_dir_of(component)),
    }
}

/// 构建时 ctx.tree 是绝对路径，源码根也要绝对化；plan 仍用相对路径。
fn absolute_source(ctx: &Ctx<'_>, relative: &str) -> String {
    let tree = std::path::Path::new(ctx.tree);
    if !tree.is_absolute() {
        return relative.to_owned();
    }
    let Some(root) = tree.parent().and_then(|p| p.parent()) else {
        return relative.to_owned();
    };
    root.join(relative).display().to_string()
}

/// bundled 组件在产品源码树里的相对路径：组件声明优先，否则用产品 profile 的模板。
pub(crate) fn bundled_source_path(product: &Target, component: &Target) -> Result<String> {
    if !component.source_path.is_empty() {
        return Ok(component.source_path.clone());
    }
    if product.profile.bundled_path.is_empty() {
        bail!(
            "component:{} 是 bundled，但没有 source_path，产品 {} 的 profile 也没有 bundled_path",
            component.name,
            product.name
        );
    }
    Ok(product
        .profile
        .bundled_path
        .replace("{name}", &component.name))
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
            "component" => {
                let line = if n.line.is_empty() {
                    String::new()
                } else {
                    format!(", line {}", n.line)
                };
                format!("  ({} {}{})", n.source, n.mode, line)
            }
            "product" => String::new(),
            _ => format!("  {}", n.version),
        };
        out.push_str(&format!("  {:>2}. {:<24}{}\n", i + 1, n.id, extra));
    }

    for (i, n) in sim.order.iter().enumerate() {
        let version = if n.kind == "product" {
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
        if !n.url.is_empty() {
            out.push_str(&format!("  source     {}\n", n.url));
        }
        if n.kind == "component" && n.source == "bundled" {
            out.push_str(&format!("  from       {}\n", n.source_path));
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
        match n.kind.as_str() {
            "component" if n.strategy == "fold" => {
                for arg in &n.configure {
                    out.push_str(&format!("  flag       {}\n", arg));
                }
                if !n.artifacts.is_empty() {
                    out.push_str(&format!("  outputs    {}\n", n.artifacts.join(", ")));
                }
            }
            "component" => {
                for step in &n.steps {
                    out.push_str(&format!("  step       {}\n", step));
                }
                if !n.artifacts.is_empty() {
                    out.push_str(&format!("  outputs    {}\n", n.artifacts.join(", ")));
                }
            }
            _ => {
                for arg in &n.configure {
                    out.push_str(&format!("  configure  {}\n", arg));
                }
                for step in &n.make {
                    out.push_str(&format!("  make       {}\n", step));
                }
                if !n.artifacts.is_empty() {
                    out.push_str(&format!("  outputs    {}\n", n.artifacts.join(", ")));
                }
            }
        }
        if !n.ini.is_empty() {
            let path = if n.ini_dir.is_empty() {
                n.ini.clone()
            } else {
                format!("{}/{}", n.ini_dir, n.ini)
            };
            out.push_str(&format!("  ini        {path}\n"));
            for line in &n.ini_lines {
                out.push_str(&format!("             {}\n", line));
            }
        }
    }
    out
}

pub fn graph_text(
    reg: &Registry,
    order: &[&Target],
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
) -> Result<String> {
    let reachable: BTreeSet<String> = order.iter().map(|t| t.id()).collect();
    let mut out = String::new();
    for t in reg.targets.values() {
        let from = t.id();
        if !reachable.contains(&from) {
            continue;
        }
        match t.kind {
            Kind::Dep => {
                for dep in &t.needs {
                    edge(&mut out, &reachable, &from, &format!("dep:{dep}"), "");
                }
                if let Some(enabled) = features.get(&t.name) {
                    for name in enabled {
                        if let Some(f) = t.features.get(name) {
                            for need in &f.needs {
                                edge(
                                    &mut out,
                                    &reachable,
                                    &from,
                                    &format!("dep:{need}"),
                                    &format!("  (feature {name})"),
                                );
                            }
                        }
                    }
                }
            }
            Kind::Component => {
                for dep in &t.needs {
                    edge(&mut out, &reachable, &from, &format!("dep:{dep}"), "");
                }
                let mode = sel.mode_of(t);
                if reg.product_mode(t, &mode)?.strategy_kind()? == Strategy::Standalone {
                    // fold 组件不单独构建，也不使用这些工具。
                    for tool in &t.tools {
                        edge(
                            &mut out,
                            &reachable,
                            &from,
                            &format!("dep:{tool}"),
                            "  (tool)",
                        );
                    }
                    if let Some(product) = reg.get(&format!(
                        "product:{}@{}",
                        t.product,
                        reg.product_any()?.version
                    )) {
                        edge(&mut out, &reachable, &from, &product.id(), "  (standalone)");
                    }
                }
            }
            Kind::Product => {
                for name in &t.components {
                    let component = reg.component(name)?;
                    let mode = sel.mode_of(component);
                    if reg.product_mode(component, &mode)?.strategy_kind()? == Strategy::Fold {
                        edge(
                            &mut out,
                            &reachable,
                            &from,
                            &format!("component:{name}"),
                            "",
                        );
                    }
                }
            }
        }
    }
    Ok(out)
}

/// 只打印本次构建闭包内的边，避免把其它产品的依赖混进来。
fn edge(out: &mut String, reachable: &BTreeSet<String>, from: &str, to: &str, note: &str) {
    if reachable.contains(from) && reachable.contains(to) {
        out.push_str(&format!("{from} -> {to}{note}\n"));
    }
}

pub fn component_list_text(
    reg: &Registry,
    state: &ProjectState,
    installed_only: bool,
    pending_only: bool,
) -> String {
    let mut out = format!(
        "{:<16} {:<9} {:<18} {:<10} {}\n",
        "name", "source", "modes", "version", "status"
    );
    for component in reg.of_kind(Kind::Component) {
        let status = state.component_status(&component.name);
        if installed_only && status != "installed" {
            continue;
        }
        if pending_only && status != "pending" {
            continue;
        }
        out.push_str(&format!(
            "{:<16} {:<9} {:<18} {:<10} {}\n",
            component.name,
            component.source,
            component.modes.join(","),
            component.version,
            status
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
            "tree" | "product" => ctx.tree.to_owned(),
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
            _ if key.starts_with("source:") => {
                let name = &key[7..];
                let product = reg.product_any()?;
                let target = reg
                    .get(&format!("dep:{name}"))
                    .or_else(|| reg.get(&format!("component:{name}")))
                    .ok_or_else(|| anyhow!("占位符 {{source:{name}}} 找不到 dep 或 component"))?;
                absolute_source(ctx, &component_source(reg, product, target)?)
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
    use crate::recipe::{Feature, Profile, ProfileMode};

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

    fn openssl() -> Target {
        Target {
            kind: Kind::Dep,
            name: "openssl".into(),
            version: "3.5.9".into(),
            url: "https://example.com/openssl.tar.gz".into(),
            ..blank()
        }
    }

    fn openssl_component() -> Target {
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
        profile.bundled_path = "ext/{name}".into();
        Target {
            kind: Kind::Component,
            name: "openssl".into(),
            product: "php".into(),
            source: "bundled".into(),
            modes: vec!["static".into(), "shared".into()],
            default_mode: "static".into(),
            version: "8.5.12".into(),
            modules: vec!["openssl".into()],
            needs: vec!["openssl".into()],
            configure: vec!["--with-openssl={dep:openssl}".into()],
            ..blank()
        }
    }

    fn product(components: &[&str]) -> Target {
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
        profile.bundled_path = "ext/{name}".into();
        Target {
            kind: Kind::Product,
            name: "php".into(),
            version: "8.5.12".into(),
            url: "https://example.com/php.tar.xz".into(),
            series: "8.5".into(),
            bundled: vec!["openssl".into()],
            bundled_disable: BTreeMap::from([(
                "openssl".to_owned(),
                "--without-openssl".to_owned(),
            )]),
            components: components.iter().map(|s| s.to_string()).collect(),
            profile,
            ..blank()
        }
    }

    #[test]
    fn product_gets_one_side_of_the_fold_flags() {
        let reg =
            Registry::from_targets(vec![openssl(), openssl_component(), product(&[])]).unwrap();
        let product_target = reg.product_any().unwrap();
        let ctx = Ctx {
            tree: "/t",
            build: "/b".into(),
            prefix: "/t".into(),
        };
        let sel = Selection::default();
        let args = product_configure_flags(&reg, product_target, &sel, &ctx).unwrap();
        assert!(args.contains(&"--without-openssl".to_owned()));
        assert!(!args.iter().any(|arg| arg.starts_with("--with-openssl")));

        let reg =
            Registry::from_targets(vec![openssl(), openssl_component(), product(&["openssl"])])
                .unwrap();
        let product_target = reg.product_any().unwrap();
        let sel = Selection::default();
        let args = product_configure_flags(&reg, product_target, &sel, &ctx).unwrap();
        assert!(args.contains(&"--with-openssl=/t/deps/openssl-3.5.9".to_owned()));
        assert!(!args.contains(&"--without-openssl".to_owned()));
    }

    #[test]
    fn graph_only_shows_the_current_closure() {
        let unrelated = Target {
            kind: Kind::Dep,
            name: "luajit".into(),
            version: "2.1".into(),
            ..blank()
        };
        let resty = Target {
            kind: Kind::Dep,
            name: "lua-resty-core".into(),
            version: "0.1".into(),
            needs: vec!["luajit".into()],
            ..blank()
        };
        let autoconf = Target {
            kind: Kind::Dep,
            name: "autoconf".into(),
            version: "2.72".into(),
            ..blank()
        };
        let mut component = openssl_component();
        component.tools = vec!["autoconf".into()];
        let mut prod = product(&["openssl"]);
        prod.needs = vec!["autoconf".into()];
        let reg =
            Registry::from_targets(vec![openssl(), component, prod, autoconf, resty, unrelated])
                .unwrap();
        let sel = Selection::default();
        let features = BTreeMap::new();
        let order = crate::plan::resolve(&reg, "product:php@8.5.12", &sel, &features).unwrap();
        let text = graph_text(&reg, &order, &sel, &features).unwrap();

        assert!(text.contains("component:openssl -> dep:openssl"));
        assert!(text.contains("product:php@8.5.12 -> component:openssl"));
        assert!(!text.contains("component:openssl -> dep:autoconf"));
        assert!(!text.contains("lua-resty-core"));
        assert!(!text.contains("luajit"));
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
