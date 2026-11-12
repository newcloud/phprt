use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, anyhow, bail};

use crate::recipe::{Kind, Mode, Registry, Selection, Target};

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
        Kind::Ext => {
            out.extend(t.needs.iter().map(|n| format!("dep:{n}")));
        }
        Kind::Php => {
            // 只有 static 扩展进 PHP 本体的 configure；
            // shared 扩展反过来依赖 php（在后面追加）。
            for name in &t.exts {
                let ext = reg.ext(name)?;
                if sel.mode_of(ext)? == Mode::Static {
                    out.push(format!("ext:{name}"));
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
/// shared 扩展在 php 之后构建：builtin 要拿 PHP 源码里的 ext/<名字>，
/// PECL 要等 phpize / php-config 就位。
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
    if root_target.kind == Kind::Php {
        for name in &root_target.exts {
            let ext = reg.ext(name)?;
            if sel.mode_of(ext)? == Mode::Shared {
                visit(
                    reg,
                    sel,
                    features,
                    &format!("ext:{name}"),
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
    use crate::recipe::{Feature, SubDef, Target};

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
        }
    }

    fn builtin(name: &str, needs: &[&str]) -> Target {
        Target {
            kind: Kind::Ext,
            name: name.into(),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            modules: vec![name.into()],
            origin: "builtin".into(),
            link: vec!["static".into(), "shared".into()],
            shared_from: format!("ext/{name}"),
            ini: format!("20-{name}.ini"),
            ..blank()
        }
    }

    fn pecl(name: &str) -> Target {
        Target {
            kind: Kind::Ext,
            name: name.into(),
            version: "1.0".into(),
            url: format!("https://pecl.php.net/get/{name}-1.0.tgz"),
            modules: vec![name.into()],
            origin: "pecl".into(),
            link: vec!["shared".into()],
            ini: format!("20-{name}.ini"),
            ..blank()
        }
    }

    fn php(exts: &[&str]) -> Target {
        Target {
            kind: Kind::Php,
            name: "8.5.12".into(),
            version: "8.5.12".into(),
            url: "https://example.com/php.tar.gz".into(),
            exts: exts.iter().map(|s| s.to_string()).collect(),
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
            exts: vec![],
            modules: vec![],
            origin: "builtin".into(),
            link: vec![],
            subs: Default::default(),
            shared_from: String::new(),
            ini: String::new(),
            ini_lines: vec![],
            features: Default::default(),
            configure: vec![],
            make: vec![],
            artifacts: vec![],
            desc: String::new(),
        }
    }

    fn fixture() -> Registry {
        let mut libcurl = dep("libcurl", &[]);
        libcurl.features.insert("ssl".into(), feature(&["openssl"]));
        libcurl
            .features
            .insert("nghttp2".into(), feature(&["nghttp2"]));

        let mut curl = builtin("curl", &["libcurl"]);
        curl.subs.insert(
            "curl".into(),
            SubDef {
                dep: "libcurl".into(),
                configure: vec![],
            },
        );

        let mut swoole = pecl("swoole");
        swoole.needs = vec!["libcurl".into()];
        swoole.subs.insert(
            "curl".into(),
            SubDef {
                dep: "libcurl".into(),
                configure: vec!["--enable-swoole-curl".into()],
            },
        );

        Registry::from_targets(vec![
            dep("openssl", &[]),
            dep("nghttp2", &[]),
            libcurl,
            builtin("openssl", &["openssl"]),
            curl,
            swoole,
            pecl("xdebug"),
            php(&["openssl", "curl", "swoole", "xdebug"]),
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
    fn defaults_are_builtin_static_and_pecl_shared() {
        let reg = fixture();
        let sel = Selection::parse(&reg, &[]).unwrap();
        let features = sel.features(&reg);
        let order = ids(&resolve(&reg, "php:8.5.12", &sel, &features).unwrap());
        assert!(pos(&order, "ext:openssl") < pos(&order, "php:8.5.12"));
        assert!(pos(&order, "ext:curl") < pos(&order, "php:8.5.12"));
        assert!(pos(&order, "php:8.5.12") < pos(&order, "ext:swoole"));
        assert!(pos(&order, "php:8.5.12") < pos(&order, "ext:xdebug"));
    }

    #[test]
    fn explicit_modes_are_honored() {
        let reg = fixture();
        let sel = selection(&reg, &["openssl", "curl=link:shared", "swoole=link:shared"]);
        let features = sel.features(&reg);
        let order = ids(&resolve(&reg, "php:8.5.12", &sel, &features).unwrap());
        assert!(pos(&order, "ext:openssl") < pos(&order, "php:8.5.12"));
        assert!(pos(&order, "php:8.5.12") < pos(&order, "ext:curl"));
        assert!(pos(&order, "php:8.5.12") < pos(&order, "ext:swoole"));
    }

    #[test]
    fn static_pecl_is_rejected_at_parse() {
        let reg = fixture();
        let err = Selection::parse(&reg, &["xdebug=link:static".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("ext:xdebug"), "{err}");
        assert!(err.contains("契约不支持 static"), "{err}");
    }

    #[test]
    fn subs_flow_to_the_dep() {
        let reg = fixture();
        let sel = selection(
            &reg,
            &[
                "curl=link:shared,sub:curl[nghttp2,ssl]",
                "swoole=sub:curl[ssl]",
            ],
        );
        let features = sel.features(&reg);
        assert_eq!(features["libcurl"], vec!["nghttp2", "ssl"]);

        let order = ids(&resolve(&reg, "php:8.5.12", &sel, &features).unwrap());
        assert!(pos(&order, "dep:nghttp2") < pos(&order, "dep:libcurl"));
        assert!(pos(&order, "dep:libcurl") < pos(&order, "ext:curl"));
        assert!(pos(&order, "dep:libcurl") < pos(&order, "ext:swoole"));
    }

    #[test]
    fn swoole_without_sub_does_not_forward() {
        let reg = fixture();
        let sel = selection(&reg, &["swoole=link:shared"]);
        assert!(sel.features(&reg).is_empty());

        let sel = selection(&reg, &["swoole=sub:curl"]);
        assert!(sel.features(&reg).is_empty());
        let swoole = reg.ext("swoole").unwrap();
        let subs: Vec<&String> = sel.subs_of(swoole).map(|(name, _)| name).collect();
        assert_eq!(subs, vec!["curl"]);
    }

    #[test]
    fn unknown_sub_is_rejected() {
        let reg = fixture();
        let err = Selection::parse(&reg, &["curl=sub:nope".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("没有子配置 nope"), "{err}");

        let err = Selection::parse(&reg, &["curl=sub:curl[brotli]".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("契约没有 feature brotli"), "{err}");
    }

    #[test]
    fn detects_cycles() {
        let reg =
            Registry::from_targets(vec![dep("a", &["b"]), dep("b", &["a"]), php(&[])]).unwrap();
        let sel = Selection::default();
        let err = resolve(&reg, "dep:a", &sel, &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("依赖成环"), "{err}");
        assert!(err.contains("dep:a -> dep:b -> dep:a"), "{err}");
    }

    #[test]
    fn rejects_missing_dependency() {
        let err = Registry::from_targets(vec![builtin("curl", &["nope"])]).unwrap_err();
        assert!(err.to_string().contains("dep:nope"), "{err}");
    }

    #[test]
    fn bare_name_must_be_unambiguous() {
        let reg =
            Registry::from_targets(vec![dep("openssl", &[]), builtin("openssl", &[]), php(&[])])
                .unwrap();
        let sel = Selection::default();
        let err = resolve(&reg, "openssl", &sel, &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("有歧义"), "{err}");
        assert!(resolve(&reg, "dep:openssl", &sel, &BTreeMap::new()).is_ok());
    }
}
