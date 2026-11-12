use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::recipe::{Kind, Mode, Registry, Selection, Target};

/// 状态按 PHP 系列存放，升级补丁版本不会丢方案。
pub fn series_of(version: &str) -> String {
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) if !major.is_empty() && !minor.is_empty() => {
            format!("{major}.{minor}")
        }
        _ => version.to_owned(),
    }
}

pub fn state_path(root: &Path, series: &str) -> PathBuf {
    root.join(".phprt")
        .join(format!("php-{series}"))
        .join("state.json")
}

fn state_version() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtState {
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub sub: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DepState {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub features: Vec<String>,
}

/// `.phprt/php-<系列>/state.json`：当前构建方案和已编译产物的状态。
///
/// 这里只记录状态，不锁定版本；dep 的版本仍然按系列取最新。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    #[serde(default = "state_version")]
    pub version: u32,
    /// 是否已经写过状态。写过之后 ext 列表就是权威方案，即使为空。
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub php: String,
    #[serde(default)]
    pub ext: BTreeMap<String, ExtState>,
    #[serde(default)]
    pub deps: BTreeMap<String, DepState>,
}

impl Default for ProjectState {
    fn default() -> Self {
        Self {
            version: state_version(),
            active: false,
            php: String::new(),
            ext: BTreeMap::new(),
            deps: BTreeMap::new(),
        }
    }
}

impl ProjectState {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            fs::read_to_string(path).with_context(|| format!("读不到 {}", path.display()))?;
        let mut state: Self = serde_json::from_str(&text)
            .with_context(|| format!("{} 不是合法的 state.json", path.display()))?;
        state.active = true;
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        write_atomic(path, text.as_bytes())
    }

    pub fn validate(&self, reg: &Registry) -> Result<()> {
        for (name, state) in &self.ext {
            let ext = reg
                .ext(name)
                .with_context(|| format!("state.json 里的 ext:{name} 不存在"))?;
            if !state.link.is_empty() {
                let mode = Mode::parse(&state.link)?;
                if !ext.link.iter().any(|m| m == mode.as_str()) {
                    bail!(
                        "state.json 里 ext:{name} 的 link={} 不受契约支持",
                        state.link
                    );
                }
            }
            for (sub, feats) in &state.sub {
                let def = ext
                    .subs
                    .get(sub)
                    .ok_or_else(|| anyhow::anyhow!("state.json 里 ext:{name} 没有子配置 {sub}"))?;
                if feats.is_empty() {
                    continue;
                }
                let dep = reg.get(&format!("dep:{}", def.dep)).ok_or_else(|| {
                    anyhow::anyhow!("state.json 里 ext:{name} 的子配置 {sub} 没绑定依赖")
                })?;
                for f in feats {
                    if !dep.features.contains_key(f) {
                        bail!("state.json 里 dep:{} 没有 feature {f}", dep.name);
                    }
                }
            }
        }
        for (name, dep) in &self.deps {
            let target = reg
                .get(&format!("dep:{name}"))
                .ok_or_else(|| anyhow::anyhow!("state.json 里的 dep:{name} 不存在"))?;
            for f in &dep.features {
                if !target.features.contains_key(f) {
                    bail!("state.json 里 dep:{name} 没有 feature {f}");
                }
            }
        }
        Ok(())
    }

    /// 第一次写状态时，用 php recipe 的默认扩展集补种。
    pub fn ensure_seeded(&mut self, reg: &Registry) -> Result<()> {
        if self.active {
            return Ok(());
        }
        let php = reg.php()?;
        let names = php.exts.clone();
        for name in names {
            let ext = reg.ext(&name)?;
            self.ext.insert(
                name,
                ExtState {
                    link: ext.default_mode()?.as_str().to_owned(),
                    installed: false,
                    sub: BTreeMap::new(),
                },
            );
        }
        self.active = true;
        Ok(())
    }

    /// 状态里的扩展列表就是构建方案；没写过状态时沿用 php.exts。
    pub fn apply_to_registry(&self, reg: &mut Registry) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let names: Vec<String> = self.ext.keys().cloned().collect();
        for name in &names {
            reg.ext(name)?;
        }
        let php_id = reg.php()?.id();
        reg.targets
            .get_mut(&php_id)
            .expect("php 目标刚刚检查过")
            .exts = names;
        Ok(())
    }

    /// 把状态里的 link/sub 和命令行选择合并成一次求解用的选择。
    pub fn effective_selection(&self, reg: &Registry, cli: &Selection) -> Result<Selection> {
        let php = reg.php()?;
        let names: Vec<String> = if self.active {
            self.ext.keys().cloned().collect()
        } else {
            php.exts.clone()
        };
        let mut out = Selection::default();
        for name in names {
            reg.ext(&name)?;
            let mut spec = cli.per_ext.get(&name).cloned().unwrap_or_default();
            if let Some(state) = self.ext.get(&name) {
                if spec.link.is_none() && !state.link.is_empty() {
                    spec.link = Some(Mode::parse(&state.link)?);
                }
                for (sub, feats) in &state.sub {
                    let entry = spec.subs.entry(sub.clone()).or_default();
                    for f in feats {
                        if !entry.contains(f) {
                            entry.push(f.clone());
                        }
                    }
                }
            }
            out.per_ext.insert(name, spec);
        }
        Ok(out)
    }

    pub fn add_ext(&mut self, reg: &Registry, name: &str) -> Result<bool> {
        if self.ext.contains_key(name) {
            return Ok(false);
        }
        let ext = reg.ext(name)?;
        self.ext.insert(
            name.to_owned(),
            ExtState {
                link: ext.default_mode()?.as_str().to_owned(),
                installed: false,
                sub: BTreeMap::new(),
            },
        );
        self.active = true;
        Ok(true)
    }

    pub fn remove_ext(&mut self, name: &str) -> bool {
        self.ext.remove(name).is_some()
    }

    pub fn set_installed_all(&mut self) {
        for ext in self.ext.values_mut() {
            ext.installed = true;
        }
    }

    /// 把本次求解使用的 link/sub 写回状态，保留 installed。
    pub fn sync_selection(&mut self, reg: &Registry, sel: &Selection) -> Result<()> {
        for (name, spec) in &sel.per_ext {
            let ext = reg.ext(name)?;
            let entry = self.ext.entry(name.clone()).or_default();
            entry.link = sel.mode_of(ext)?.as_str().to_owned();
            entry.sub = spec.subs.clone();
        }
        // 方案之外的扩展不留在状态里。
        let keep: Vec<String> = sel.per_ext.keys().cloned().collect();
        self.ext.retain(|name, _| keep.contains(name));
        self.active = true;
        Ok(())
    }

    pub fn merged_features(
        &self,
        current: BTreeMap<String, Vec<String>>,
    ) -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = self
            .deps
            .iter()
            .map(|(name, dep)| (name.clone(), dep.features.clone()))
            .collect();
        for (name, feats) in current {
            let entry = out.entry(name).or_default();
            for f in feats {
                if !entry.contains(&f) {
                    entry.push(f);
                }
            }
        }
        for feats in out.values_mut() {
            feats.sort();
            feats.dedup();
        }
        out.retain(|_, feats| !feats.is_empty());
        out
    }

    /// 用本次闭包重建 deps 状态；版本只记录，不参与锁定。
    pub fn set_deps(&mut self, order: &[&Target], features: &BTreeMap<String, Vec<String>>) {
        self.deps = order
            .iter()
            .filter(|t| t.kind == Kind::Dep)
            .map(|t| {
                (
                    t.name.clone(),
                    DepState {
                        version: t.version.clone(),
                        features: features.get(&t.name).cloned().unwrap_or_default(),
                    },
                )
            })
            .collect();
    }

    pub fn ext_status(&self, name: &str) -> &'static str {
        match self.ext.get(name) {
            Some(e) if e.installed => "installed",
            Some(_) => "pending",
            None => "available",
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("建不了目录 {}", dir.display()))?;
    }
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("state");
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    fs::write(&tmp, bytes).with_context(|| format!("写不了 {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("替换不了 {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{SubDef, Target};

    fn dep(name: &str, features: &[&str]) -> Target {
        let mut t = Target {
            kind: Kind::Dep,
            name: name.into(),
            version: "1.0".into(),
            url: "https://example.com/x.tar.gz".into(),
            ..blank()
        };
        for f in features {
            t.features.insert(
                (*f).into(),
                crate::recipe::Feature {
                    needs: vec![],
                    configure: vec![],
                },
            );
        }
        t
    }

    fn ext(name: &str) -> Target {
        let mut t = Target {
            kind: Kind::Ext,
            name: name.into(),
            modules: vec![name.into()],
            origin: "builtin".into(),
            link: vec!["static".into(), "shared".into()],
            shared_from: format!("ext/{name}"),
            ini: format!("20-{name}.ini"),
            ..blank()
        };
        t.subs.insert(
            "curl".into(),
            SubDef {
                dep: "libcurl".into(),
                configure: vec![],
            },
        );
        t
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

    fn registry() -> Registry {
        let mut curl = ext("curl");
        curl.needs = vec!["libcurl".into()];
        let mut swoole = ext("swoole");
        swoole.needs = vec!["libcurl".into()];
        swoole.origin = "pecl".into();
        swoole.link = vec!["shared".into()];
        let php = Target {
            kind: Kind::Php,
            name: "8.5.12".into(),
            version: "8.5.12".into(),
            url: "https://example.com/php.tar.gz".into(),
            exts: vec!["curl".into(), "swoole".into()],
            ..blank()
        };
        Registry::from_targets(vec![dep("libcurl", &["ssl", "nghttp2"]), curl, swoole, php])
            .unwrap()
    }

    #[test]
    fn series_of_takes_major_minor() {
        assert_eq!(series_of("8.5.12"), "8.5");
        assert_eq!(series_of("8.6.0-dev"), "8.6");
        assert_eq!(series_of("8"), "8");
    }

    #[test]
    fn seed_uses_php_exts_once() {
        let reg = registry();
        let mut state = ProjectState::default();
        state.ensure_seeded(&reg).unwrap();
        assert_eq!(
            state.ext.keys().cloned().collect::<Vec<_>>(),
            vec!["curl", "swoole"]
        );
        assert!(state.active);
        state.remove_ext("swoole");
        state.ensure_seeded(&reg).unwrap();
        assert!(!state.ext.contains_key("swoole"));
    }

    #[test]
    fn merged_features_never_shrink() {
        let mut state = ProjectState::default();
        state.deps.insert(
            "libcurl".into(),
            DepState {
                version: "1.0".into(),
                features: vec!["nghttp2".into()],
            },
        );
        let merged = state.merged_features(BTreeMap::from([(
            "libcurl".to_string(),
            vec!["ssl".to_string()],
        )]));
        assert_eq!(merged["libcurl"], vec!["nghttp2", "ssl"]);
    }

    #[test]
    fn effective_selection_carries_state_subs() {
        let reg = registry();
        let mut state = ProjectState::default();
        state.ensure_seeded(&reg).unwrap();
        state
            .ext
            .get_mut("curl")
            .unwrap()
            .sub
            .insert("curl".into(), vec!["nghttp2".into()]);
        let cli = Selection::default();
        let sel = state.effective_selection(&reg, &cli).unwrap();
        assert_eq!(sel.features(&reg)["libcurl"], vec!["nghttp2"]);
        assert_eq!(
            sel.mode_of(reg.ext("swoole").unwrap()).unwrap(),
            Mode::Shared
        );
    }

    #[test]
    fn status_reflects_installed_flag() {
        let reg = registry();
        let mut state = ProjectState::default();
        state.ensure_seeded(&reg).unwrap();
        assert_eq!(state.ext_status("curl"), "pending");
        state.set_installed_all();
        assert_eq!(state.ext_status("curl"), "installed");
        state.remove_ext("curl");
        assert_eq!(state.ext_status("curl"), "available");
    }
}
