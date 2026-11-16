use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, anyhow, bail};

use crate::recipe::{Kind, Registry, Selection, Strategy, Target};

fn strategy(reg: &Registry, component: &Target, sel: &Selection) -> Result<Strategy> {
    let mode = sel.mode_of(component);
    reg.product_mode(component, &mode)?.strategy_kind()
}

fn children(
    reg: &Registry,
    t: &Target,
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    match t.kind {
        Kind::Dep => {
            out.extend(t.needs.iter().map(|n| format!("dep:{n}")));
            if let Some(enabled) = features.get(&t.name) {
                for name in enabled {
                    if let Some(f) = t.features.get(name) {
                        out.extend(f.needs.iter().map(|n| format!("dep:{n}")));
                    }
                }
            }
        }
        Kind::Component => {
            out.extend(t.needs.iter().map(|n| format!("dep:{n}")));
            // 选中的 feature 可能额外需要一个依赖库（如 swoole 的 cares）。
            out.extend(sel.feature_needs(t).into_iter().map(|n| format!("dep:{n}")));
            // 只有 standalone 组件需要 phpize / autoconf 这类构建工具。
            if strategy(reg, t, sel)? == Strategy::Standalone {
                out.extend(t.tools.iter().map(|n| format!("dep:{n}")));
            }
        }
        Kind::Product => {
            out.extend(t.needs.iter().map(|n| format!("dep:{n}")));
            // 只有 fold 组件进产品本体的 configure；
            // standalone 组件反过来依赖产品（在后面追加）。
            for name in &t.components {
                let component = reg.component(name)?;
                if strategy(reg, component, sel)? == Strategy::Fold {
                    out.push(format!("component:{name}"));
                }
            }
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn visit<'a>(
    reg: &'a Registry,
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
    id: &str,
    state: &mut HashMap<String, u8>,
    stack: &mut Vec<String>,
    out: &mut Vec<&'a Target>,
) -> Result<()> {
    match state.get(id).copied().unwrap_or(0) {
        2 => return Ok(()),
        1 => {
            let pos = stack.iter().position(|s| s == id).unwrap_or(0);
            let mut cycle = stack[pos..].to_vec();
            cycle.push(id.to_owned());
            bail!("依赖成环: {}", cycle.join(" -> "));
        }
        _ => {}
    }

    let target = reg.get(id).ok_or_else(|| anyhow!("未知目标 {id}"))?;
    state.insert(id.to_owned(), 1);
    stack.push(id.to_owned());
    for child in children(reg, target, sel, features)? {
        visit(reg, sel, features, &child, state, stack, out)?;
    }
    stack.pop();
    state.insert(id.to_owned(), 2);
    out.push(target);
    Ok(())
}

/// 返回依赖优先的构建顺序（拓扑序）。
///
/// standalone 组件在产品之后构建：bundled 要拿产品源码里的组件目录，
/// external 要等产品的 phpize / python / nginx 就位。
pub fn resolve<'a>(
    reg: &'a Registry,
    root: &str,
    sel: &Selection,
    features: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<&'a Target>> {
    let id = reg.resolve_name(root)?;
    let mut out = Vec::new();
    let mut state = HashMap::new();
    let mut stack = Vec::new();
    visit(reg, sel, features, &id, &mut state, &mut stack, &mut out)?;

    let root_target = reg.get(&id).ok_or_else(|| anyhow!("未知目标 {id}"))?;
    if root_target.kind == Kind::Product {
        for name in &root_target.components {
            let component = reg.component(name)?;
            if strategy(reg, component, sel)? == Strategy::Standalone {
                visit(
                    reg,
                    sel,
                    features,
                    &format!("component:{name}"),
                    &mut state,
                    &mut stack,
                    &mut out,
                )?;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{Feature, Profile, ProfileMode};

    fn dep(name: &str, needs: &[&str]) -> Target {
        Target {
            kind: Kind::Dep,
            name: name.into(),
            version: "1".into(),
            url: "https://example.com/x.tar.gz".into(),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            ..blank()
        }
    }

    fn feature(needs: &[&str]) -> Feature {
        Feature {
            needs: needs.iter().map(|s| s.to_string()).collect(),
            configure: vec![],
            disable: vec![],
            requires: Default::default(),
        }
    }

    fn bundled(name: &str, needs: &[&str]) -> Target {
        Target {
            kind: Kind::Component,
            name: name.into(),
            product: "php".into(),
            source: "bundled".into(),
            modes: vec!["static".into(), "shared".into()],
            default_mode: "static".into(),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            modules: vec![name.into()],
            ini: format!("20-{name}.ini"),
            ..blank()
        }
    }

    fn tool(name: &str) -> Target {
        Target {
            kind: Kind::Dep,
            name: name.into(),
            version: "1".into(),
            url: format!("https://example.com/{name}.tar.gz"),
            artifacts: vec!["bin/x".into()],
            ..blank()
        }
    }

    fn external(name: &str) -> Target {
        Target {
            kind: Kind::Component,
            name: name.into(),
            product: "php".into(),
            source: "external".into(),
            modes: vec!["shared".into()],
            default_mode: "shared".into(),
            version: "1.0".into(),
            url: format!("https://pecl.php.net/get/{name}-1.0.tgz"),
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
            bundled: vec!["openssl".into(), "curl".into()],
            components: components.iter().map(|s| s.to_string()).collect(),
            profile: profile(),
            ..blank()
        }
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

    fn fixture() -> Registry {
        let mut libcurl = dep("libcurl", &[]);
        libcurl.features.insert("ssl".into(), feature(&["openssl"]));
        libcurl
            .features
            .insert("nghttp2".into(), feature(&["nghttp2"]));

        let mut curl = bundled("curl", &["libcurl"]);
        curl.tools = vec!["autoconf".into()];
        curl.dep_configure.insert("libcurl".into(), vec![]);

        let mut swoole = external("swoole");
        swoole.needs = vec!["libcurl".into()];
        swoole
            .dep_configure
            .insert("libcurl".into(), vec!["--enable-swoole-curl".into()]);

        Registry::from_targets(vec![
            dep("openssl", &[]),
            dep("nghttp2", &[]),
            tool("autoconf"),
            libcurl,
            bundled("openssl", &["openssl"]),
            curl,
            swoole,
            external("xdebug"),
            product(&["openssl", "curl", "swoole", "xdebug"]),
        ])
        .unwrap()
    }

    fn selection(reg: &Registry, specs: &[&str]) -> Selection {
        let specs: Vec<String> = specs.iter().map(|s| s.to_string()).collect();
        Selection::parse(reg, &specs).unwrap()
    }

    fn ids(order: &[&Target]) -> Vec<String> {
        order.iter().map(|t| t.id()).collect()
    }

    fn pos(order: &[String], id: &str) -> usize {
        order.iter().position(|s| s == id).unwrap()
    }

    #[test]
    fn defaults_are_fold_for_bundled_and_standalone_for_external() {
        let reg = fixture();
        let sel = Selection::parse(&reg, &[]).unwrap();
        let features = sel.features(&reg);
        let order = ids(&resolve(&reg, "product:php@8.5.12", &sel, &features).unwrap());
        assert!(pos(&order, "component:openssl") < pos(&order, "product:php@8.5.12"));
        assert!(pos(&order, "component:curl") < pos(&order, "product:php@8.5.12"));
        assert!(pos(&order, "product:php@8.5.12") < pos(&order, "component:swoole"));
        assert!(pos(&order, "product:php@8.5.12") < pos(&order, "component:xdebug"));
    }

    #[test]
    fn explicit_modes_are_honored() {
        let reg = fixture();
        let sel = selection(&reg, &["openssl", "curl=mode:shared", "swoole=mode:shared"]);
        let features = sel.features(&reg);
        let order = ids(&resolve(&reg, "product:php@8.5.12", &sel, &features).unwrap());
        assert!(pos(&order, "component:openssl") < pos(&order, "product:php@8.5.12"));
        assert!(pos(&order, "product:php@8.5.12") < pos(&order, "component:curl"));
        assert!(pos(&order, "product:php@8.5.12") < pos(&order, "component:swoole"));
    }

    #[test]
    fn unknown_mode_is_rejected_at_parse() {
        let reg = fixture();
        let err = Selection::parse(&reg, &["xdebug=mode:static".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("component:xdebug"), "{err}");
        assert!(err.contains("不支持 mode:static"), "{err}");
    }

    #[test]
    fn tools_only_follow_standalone_components() {
        let reg = fixture();
        let sel = selection(&reg, &["curl=mode:static"]);
        let features = sel.features(&reg);
        let order = ids(&resolve(&reg, "product:php@8.5.12", &sel, &features).unwrap());
        assert!(!order.contains(&"dep:autoconf".to_owned()));

        let sel = selection(&reg, &["curl=mode:shared"]);
        let features = sel.features(&reg);
        let order = ids(&resolve(&reg, "product:php@8.5.12", &sel, &features).unwrap());
        assert!(pos(&order, "dep:autoconf") < pos(&order, "component:curl"));
    }

    #[test]
    fn deps_flow_to_the_dep() {
        let reg = fixture();
        let sel = selection(
            &reg,
            &[
                "curl=mode:shared,dep:libcurl[nghttp2,ssl]",
                "swoole=dep:libcurl[ssl]",
            ],
        );
        let features = sel.features(&reg);
        assert_eq!(features["libcurl"], vec!["nghttp2", "ssl"]);

        let order = ids(&resolve(&reg, "product:php@8.5.12", &sel, &features).unwrap());
        assert!(pos(&order, "dep:nghttp2") < pos(&order, "dep:libcurl"));
        assert!(pos(&order, "dep:libcurl") < pos(&order, "component:curl"));
        assert!(pos(&order, "dep:libcurl") < pos(&order, "component:swoole"));
    }

    #[test]
    fn swoole_without_dep_does_not_forward() {
        let reg = fixture();
        let sel = selection(&reg, &["swoole=mode:shared"]);
        assert!(sel.features(&reg).is_empty());

        let sel = selection(&reg, &["swoole=dep:libcurl"]);
        assert!(sel.features(&reg).is_empty());
        let swoole = reg.component("swoole").unwrap();
        let deps: Vec<&String> = sel.deps_of(swoole).map(|(name, _)| name).collect();
        assert_eq!(deps, vec!["libcurl"]);
    }

    #[test]
    fn component_feature_needs_join_the_build_order() {
        let mut reg = fixture();
        reg.targets.insert("dep:c-ares".into(), dep("c-ares", &[]));
        reg.targets
            .get_mut("component:swoole")
            .unwrap()
            .features
            .insert(
                "cares".into(),
                Feature {
                    needs: vec!["c-ares".into()],
                    ..Default::default()
                },
            );

        let sel = selection(&reg, &["swoole=feature:[cares]"]);
        let features = sel.features(&reg);
        let order = ids(&resolve(&reg, "product:php@8.5.12", &sel, &features).unwrap());
        assert!(pos(&order, "dep:c-ares") < pos(&order, "component:swoole"));

        let sel = selection(&reg, &["swoole=mode:shared"]);
        let order = ids(&resolve(&reg, "product:php@8.5.12", &sel, &BTreeMap::new()).unwrap());
        assert!(!order.contains(&"dep:c-ares".to_owned()));
    }

    #[test]
    fn unknown_dep_is_rejected() {
        let reg = fixture();
        let err = Selection::parse(&reg, &["curl=dep:nope".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("没有依赖集成 nope"), "{err}");

        let err = Selection::parse(&reg, &["curl=dep:libcurl[brotli]".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("契约没有 feature brotli"), "{err}");
    }

    #[test]
    fn detects_cycles() {
        let reg =
            Registry::from_targets(vec![dep("a", &["b"]), dep("b", &["a"]), product(&[])]).unwrap();
        let sel = Selection::default();
        let err = resolve(&reg, "dep:a", &sel, &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("依赖成环"), "{err}");
        assert!(err.contains("dep:a -> dep:b -> dep:a"), "{err}");
    }

    #[test]
    fn rejects_missing_dependency() {
        let err =
            Registry::from_targets(vec![bundled("curl", &["nope"]), product(&[])]).unwrap_err();
        assert!(err.to_string().contains("dep:nope"), "{err}");
    }

    #[test]
    fn bare_name_must_be_unambiguous() {
        let reg = Registry::from_targets(vec![
            dep("openssl", &[]),
            bundled("openssl", &[]),
            product(&[]),
        ])
        .unwrap();
        let sel = Selection::default();
        let err = resolve(&reg, "openssl", &sel, &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("有歧义"), "{err}");
        assert!(resolve(&reg, "dep:openssl", &sel, &BTreeMap::new()).is_ok());
        assert!(resolve(&reg, "product:php@8.5.12", &sel, &BTreeMap::new()).is_ok());
    }
}
