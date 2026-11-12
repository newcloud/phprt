use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use serde::Serialize;

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
    pub origin: String,
    pub mode: String,
    pub source: String,
    pub sha256: String,
    pub unpack: String,
    pub prefix: String,
    pub needs: Vec<String>,
    pub modules: Vec<String>,
    pub subs: Vec<SubNode>,
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
pub struct SubNode {
    pub name: String,
    pub dep: String,
    pub features: Vec<String>,
}

struct Ctx<'a> {
    tree: &'a str,
    build: String,
    prefix: String,
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
    let php_build = format!("build/php/{}", archive_stem(&archive_name(&php.url)));
    let mut nodes = Vec::new();

    for t in order {
        let archive = archive_name(&t.url);
        let stem = archive_stem(&archive);
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
        let mut enabled_features: Vec<String> = Vec::new();

        if t.kind == Kind::Dep
            && let Some(enabled) = features.get(&t.name)
        {
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
                for arg in &f.configure {
                    configure.push(expand(arg, &ctx, reg)?);
                }
            }
        }

        // 编进 PHP 的扩展，把参数汇总到 PHP 的 configure 行。
        if t.kind == Kind::Php {
            for ext_name in &t.exts {
                let ext = reg.ext(ext_name)?;
                if sel.mode_of(ext)? == Mode::Static {
                    let ext_ctx = Ctx {
                        tree: &tree,
                        build: build_dir.clone(),
                        prefix: tree.clone(),
                    };
                    configure.extend(ext_configure(reg, ext, sel, &ext_ctx)?);
                }
            }
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
                    unpack = String::new();
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

        let subs = if t.kind == Kind::Ext {
            sel.subs_of(t)
                .map(|(name, feats)| SubNode {
                    name: name.clone(),
                    dep: t.subs.get(name).map(|d| d.dep.clone()).unwrap_or_default(),
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
            modules: t.modules.clone(),
            subs,
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

fn ext_configure(
    reg: &Registry,
    ext: &Target,
    sel: &Selection,
    ctx: &Ctx<'_>,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for arg in &ext.configure {
        out.push(expand(arg, ctx, reg)?);
    }
    for (sub, _) in sel.subs_of(ext) {
        let Some(def) = ext.subs.get(sub) else {
            continue;
        };
        for arg in &def.configure {
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
    out.push_str("phprt plan（仅模拟，不编译）\n");
    out.push_str(&format!("  root   {}\n", sim.root));
    out.push_str(&format!("  tree   {}\n", sim.tree));

    out.push_str("\nbuild order\n");
    for (i, n) in sim.order.iter().enumerate() {
        let extra = match n.kind.as_str() {
            "ext" => format!("  ({} {})", n.origin, n.mode),
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
        if !n.needs.is_empty() {
            out.push_str(&format!("  needs      {}\n", n.needs.join(", ")));
        }
        if !n.modules.is_empty() {
            out.push_str(&format!("  modules    {}\n", n.modules.join(", ")));
        }
        for sub in &n.subs {
            let feats = if sub.features.is_empty() {
                String::new()
            } else {
                format!(" [{}]", sub.features.join(", "))
            };
            let dep = if sub.dep.is_empty() {
                String::new()
            } else {
                format!(" -> dep:{}", sub.dep)
            };
            out.push_str(&format!("  sub        {}{}{}\n", sub.name, feats, dep));
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
        "{:<12} {:<9} {:<15} {}\n",
        "name", "origin", "link", "status"
    );
    let defaults = reg.php().map(|p| p.exts.clone()).unwrap_or_default();
    for ext in reg.of_kind(Kind::Ext) {
        let status = if !state.active && defaults.contains(&ext.name) {
            "pending"
        } else {
            state.ext_status(&ext.name)
        };
        if installed_only && status != "installed" {
            continue;
        }
        if pending_only && status != "pending" {
            continue;
        }
        out.push_str(&format!(
            "{:<12} {:<9} {:<15} {}\n",
            ext.name,
            ext.origin,
            ext.link.join(","),
            status
        ));
    }
    out
}

pub fn list_text(reg: &Registry) -> String {
    let mut out = String::new();
    for t in reg.targets.values() {
        let mut notes = Vec::new();
        if !t.needs.is_empty() {
            notes.push(format!("needs={}", t.needs.join(",")));
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
            if !t.subs.is_empty() {
                let subs: Vec<String> = t
                    .subs
                    .iter()
                    .map(|(name, def)| {
                        if def.dep.is_empty() {
                            name.clone()
                        } else {
                            format!("{name}->{}", def.dep)
                        }
                    })
                    .collect();
                notes.push(format!("subs={}", subs.join(",")));
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

fn archive_name(url: &str) -> String {
    url.rsplit('/').next().unwrap_or("source.tar.gz").to_owned()
}

fn archive_stem(name: &str) -> String {
    for suffix in [".tar.gz", ".tar.xz", ".tar.bz2", ".tgz", ".zip"] {
        if let Some(stem) = name.strip_suffix(suffix) {
            return stem.to_owned();
        }
    }
    name.to_owned()
}

fn expand(input: &str, ctx: &Ctx<'_>, reg: &Registry) -> Result<String> {
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

    #[test]
    fn archive_stem_handles_common_suffixes() {
        assert_eq!(archive_stem("openssl-3.5.4.tar.gz"), "openssl-3.5.4");
        assert_eq!(archive_stem("php-8.5.12.tar.xz"), "php-8.5.12");
        assert_eq!(archive_stem("curl-8.11.1.zip"), "curl-8.11.1");
        assert_eq!(archive_stem("plain"), "plain");
    }
}
