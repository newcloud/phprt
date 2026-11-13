use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::recipe::{Kind, Mode, Registry, Selection, Target};

pub fn state_path(root: &Path, series: &str) -> PathBuf {
    root.join(".phprt")
        .join(format!("php-{series}"))
        .join("state.json")
}

fn state_version() -> u32 {
    2
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtState {
    /// 选中的版本线，如 6.2；builtin 扩展为空。
    #[serde(default)]
    pub line: String,
    /// 实际解析到的补丁，如 6.2.5；只记录，不锁定。
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub installed: bool,
    /// 依赖名 -> 转给该依赖的 feature。
    #[serde(default, alias = "sub")]
    pub deps: BTreeMap<String, Vec<String>>,
    /// 扩展自己的 feature。
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DepState {
    #[serde(default)]
    pub line: String,
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
    /// 用户显式指定的版本线；bundle 解析出来的默认线不写在这里。
    #[serde(default)]
    pub bindings: BTreeMap<String, String>,
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
            bindings: BTreeMap::new(),
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
            for (dep_name, feats) in &state.deps {
                if !ext.dep_configure.contains_key(dep_name) {
                    let available: Vec<&str> =
                        ext.dep_configure.keys().map(String::as_str).collect();
                    let hint = if available.is_empty() {
                        "无".to_owned()
                    } else {
                        available.join(", ")
                    };
                    bail!(
                        "state.json 里 ext:{name} 的 dep={dep_name} 不存在（可写：{hint}）；\
                         sub 已改名为 dep，旧状态需要手工迁移或删除 \
                         .phprt/php-<系列>/state.json"
                    );
                }
                if feats.is_empty() {
                    continue;
                }
                let dep = reg.get(&format!("dep:{dep_name}")).ok_or_else(|| {
                    anyhow::anyhow!("state.json 里 ext:{name} 的 dep:{dep_name} 不存在")
                })?;
                for f in feats {
                    if !dep.features.contains_key(f) {
                        bail!("state.json 里 dep:{} 没有 feature {f}", dep.name);
                    }
                }
            }
            for feature in &state.features {
                if !ext.features.contains_key(feature) {
                    let available: Vec<&str> = ext.features.keys().map(String::as_str).collect();
                    let hint = if available.is_empty() {
                        "无".to_owned()
                    } else {
                        available.join(", ")
                    };
                    bail!("state.json 里 ext:{name} 没有 feature {feature}（可用：{hint}）");
                }
                if let Err(err) = crate::recipe::feature_available(reg, ext, feature) {
                    bail!("state.json 里 {err}；改 state 或换依赖线后重试");
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

    /// 当前构建方案 = 状态里的扩展 ∪ `--ext` 点名的扩展。
    /// 两者都没有时为空，也就是只装 PHP。
    /// 当前构建方案 = 状态里的扩展 ∪ 本次命令行点名的扩展。
    /// 两者都没有时为空，也就是只装 PHP。
    pub fn scheme_with(&self, cli_names: &[String]) -> Vec<String> {
        let mut names: Vec<String> = if self.active {
            self.ext.keys().cloned().collect()
        } else {
            Vec::new()
        };
        for name in cli_names {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
        names.sort();
        names
    }

    /// 把方案写进 php.exts；求解器只认这一份，不从 recipe 取默认扩展。
    pub fn apply_scheme(&self, reg: &mut Registry, names: &[String]) -> Result<()> {
        for name in names {
            reg.ext(name)?;
        }
        let php_id = reg.php()?.id();
        reg.targets
            .get_mut(&php_id)
            .expect("php 目标刚刚检查过")
            .exts = names.to_vec();
        Ok(())
    }

    /// 把状态里的 link/sub 和命令行选择合并成一次求解用的选择。
    pub fn effective_selection(
        &self,
        reg: &Registry,
        cli: &Selection,
        names: &[String],
    ) -> Result<Selection> {
        let mut out = Selection::default();
        for name in names {
            reg.ext(name)?;
            let mut spec = cli.per_ext.get(name).cloned().unwrap_or_default();
            if let Some(state) = self.ext.get(name) {
                if spec.link.is_none() && !state.link.is_empty() {
                    spec.link = Some(Mode::parse(&state.link)?);
                }
                for (dep, feats) in &state.deps {
                    let entry = spec.deps.entry(dep.clone()).or_default();
                    for f in feats {
                        if !entry.contains(f) {
                            entry.push(f.clone());
                        }
                    }
                }
                // feature 只增不减会让默认值关不掉；CLI 显式写了就以 CLI 为准。
                if spec.features.is_none() {
                    spec.features = Some(state.features.iter().cloned().collect());
                }
            }
            out.per_ext.insert(name.clone(), spec);
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
                line: String::new(),
                version: String::new(),
                link: ext.default_mode()?.as_str().to_owned(),
                installed: false,
                deps: BTreeMap::new(),
                features: Vec::new(),
            },
        );
        self.active = true;
        Ok(true)
    }

    pub fn remove_ext(&mut self, name: &str) -> bool {
        let Some(ext) = self.ext.remove(name) else {
            return false;
        };
        // builtin 扩展（openssl/curl）没有自己的线，同名 binding 属于依赖库。
        if !ext.line.is_empty() {
            self.bindings.remove(name);
        }
        true
    }

    pub fn set_installed_all(&mut self) {
        for ext in self.ext.values_mut() {
            ext.installed = true;
        }
    }

    /// 把本次求解使用的 link/sub 写回状态，保留 installed。
    pub fn sync_selection(
        &mut self,
        reg: &Registry,
        sel: &Selection,
        names: &[String],
    ) -> Result<()> {
        for name in names {
            let ext = reg.ext(name)?;
            let spec = sel.per_ext.get(name).cloned().unwrap_or_default();
            let entry = self.ext.entry(name.clone()).or_default();
            entry.link = sel.mode_of(ext)?.as_str().to_owned();
            entry.line = ext.line.clone();
            entry.version = ext.version.clone();
            entry.deps = spec.deps;
            entry.features = spec
                .features
                .map(|set| set.into_iter().collect())
                .unwrap_or_default();
        }
        // 方案之外的扩展不留在状态里。
        self.ext.retain(|name, _| names.contains(name));
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
                        line: t.line.clone(),
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
    use crate::recipe::Target;

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
                    disable: vec![],
                    requires: Default::default(),
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
        t.dep_configure.insert("libcurl".into(), vec![]);
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
            ..blank()
        };
        Registry::from_targets(vec![dep("libcurl", &["ssl", "nghttp2"]), curl, swoole, php])
            .unwrap()
    }

    #[test]
    fn default_scheme_is_empty() {
        let state = ProjectState::default();
        assert!(state.scheme_with(&[]).is_empty());

        let mut reg = registry();
        state.apply_scheme(&mut reg, &[]).unwrap();
        assert!(reg.php().unwrap().exts.is_empty());
    }

    #[test]
    fn scheme_takes_cli_exts() {
        let state = ProjectState::default();
        assert_eq!(state.scheme_with(&["swoole".to_owned()]), vec!["swoole"]);
    }

    #[test]
    fn merged_features_never_shrink() {
        let mut state = ProjectState::default();
        state.deps.insert(
            "libcurl".into(),
            DepState {
                line: "8.23".into(),
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
    fn effective_selection_carries_state_deps() {
        let reg = registry();
        let mut state = ProjectState::default();
        state.add_ext(&reg, "curl").unwrap();
        state.add_ext(&reg, "swoole").unwrap();
        state
            .ext
            .get_mut("curl")
            .unwrap()
            .deps
            .insert("libcurl".into(), vec!["nghttp2".into()]);
        let cli = Selection::default();
        let names = state.scheme_with(&[]);
        let sel = state.effective_selection(&reg, &cli, &names).unwrap();
        assert_eq!(names, vec!["curl", "swoole"]);
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
        assert_eq!(state.ext_status("curl"), "available");
        state.add_ext(&reg, "curl").unwrap();
        assert_eq!(state.ext_status("curl"), "pending");
        state.set_installed_all();
        assert_eq!(state.ext_status("curl"), "installed");
        state.remove_ext("curl");
        assert_eq!(state.ext_status("curl"), "available");
    }
}
