use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::recipe::{ComponentSpec, Kind, Registry, Selection, Target, TargetSelection};

pub fn state_path(root: &Path, product: &str, series: &str) -> PathBuf {
    crate::layout::state_file(root, product, series)
}

fn state_version() -> u32 {
    3
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComponentState {
    /// 选中的版本线，如 6.2；bundled 组件为空。
    #[serde(default)]
    pub line: String,
    /// 实际解析到的补丁，如 6.2.5；`--lock` 把它当作精确版本。
    #[serde(default)]
    pub version: String,
    /// 挂载方式，如 static / shared / dynamic。
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub installed: bool,
    /// 依赖名 -> 转给该依赖的 feature。
    #[serde(default)]
    pub deps: BTreeMap<String, Vec<String>>,
    /// 组件自己的 feature。
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

/// 构建环境的选择（宿主 / OCI 镜像）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildState {
    /// `oci` 或 `host`。
    #[serde(default)]
    pub mode: String,
    /// 镜像 manifest digest，如 sha256:...
    #[serde(default)]
    pub image: String,
    /// 拉取时的镜像引用，只作来源记录。
    #[serde(default)]
    pub image_ref: String,
    /// 运行时名；当前只有 youki。
    #[serde(default)]
    pub runtime: String,
    /// 当前实例名；实例是用户级全局对象，名字由用户或默认规则决定。
    #[serde(default)]
    pub instance: String,
}

impl BuildState {
    pub fn is_oci(&self) -> bool {
        self.mode == "oci" && !self.image.is_empty()
    }

    pub fn signature_key(&self) -> String {
        if self.is_oci() {
            format!("oci:{}", self.image)
        } else {
            "host".to_string()
        }
    }
}

/// 一次构建实际解析到的工具；只记录，不参与方案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolState {
    pub name: String,
    #[serde(default)]
    pub version: String,
    /// `host` 或 `store`。
    pub provider: String,
}

/// `.phprt/<产品>-<系列>/state.json`：当前构建方案和已编译产物的状态。
///
/// 解析时它是 `--lock` 的版本来源：line 默认复用，`--lock` 再复用 version；
/// 真正的源码地址仍然只从 catalog 取。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    #[serde(default = "state_version")]
    pub version: u32,
    /// 是否已经写过状态。写过之后组件列表就是权威方案，即使为空。
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub series: String,
    #[serde(default)]
    pub installed_version: String,
    /// 用户显式指定的版本线；bundle 解析出来的默认线不写在这里。
    #[serde(default)]
    pub bindings: BTreeMap<String, String>,
    /// 产品构建目标：target 名 -> 配置名 -> 值。空表示沿用 bundle 默认值。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub targets: TargetSelection,
    #[serde(default)]
    pub components: BTreeMap<String, ComponentState>,
    #[serde(default)]
    pub deps: BTreeMap<String, DepState>,
    /// 上次构建实际用到的工具；`--lock` 复用其中的 built 版本。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolState>,
    /// 构建环境；没有这个字段表示宿主构建。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildState>,
}

impl Default for ProjectState {
    fn default() -> Self {
        Self {
            version: state_version(),
            active: false,
            product: String::new(),
            series: String::new(),
            installed_version: String::new(),
            bindings: BTreeMap::new(),
            targets: TargetSelection::new(),
            components: BTreeMap::new(),
            deps: BTreeMap::new(),
            tools: Vec::new(),
            build: None,
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
        if state.version != state_version() {
            bail!(
                "{} 是旧格式（version={}），当前只支持 version={}；\
                 删掉旧的 .phprt/*/state.json 并按新方案重新安装",
                path.display(),
                state.version,
                state_version()
            );
        }
        state.active = true;
        Ok(state)
    }

    /// 首次使用时写入产品/系列；已有状态必须一致。
    pub fn check(&mut self, product: &str, series: &str) -> Result<()> {
        if !self.active {
            self.product = product.to_owned();
            self.series = series.to_owned();
            return Ok(());
        }
        if self.product != product || self.series != series {
            bail!(
                "状态文件属于 {}@{}，不是 {product}@{series}",
                self.product,
                self.series
            );
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        write_atomic(path, text.as_bytes())
    }

    pub fn validate(&self, reg: &Registry) -> Result<()> {
        for (name, state) in &self.components {
            let component = reg
                .component(name)
                .with_context(|| format!("state.json 里的 component:{name} 不存在"))?;
            if !state.mode.is_empty() {
                if !component.modes.iter().any(|m| m == &state.mode) {
                    bail!(
                        "state.json 里 component:{name} 的 mode={} 不受契约支持",
                        state.mode
                    );
                }
                reg.product_mode(component, &state.mode)?;
            }
            for (dep_name, feats) in &state.deps {
                if !component.dep_configure.contains_key(dep_name) {
                    let available: Vec<&str> =
                        component.dep_configure.keys().map(String::as_str).collect();
                    let hint = if available.is_empty() {
                        "无".to_owned()
                    } else {
                        available.join(", ")
                    };
                    bail!(
                        "state.json 里 component:{name} 的 dep={dep_name} 不存在（可写：{hint}）；\
                         旧状态需要手工迁移或删除 .phprt/{}-{}/state.json",
                        self.product,
                        self.series
                    );
                }
                if feats.is_empty() {
                    continue;
                }
                let dep = reg.get(&format!("dep:{dep_name}")).ok_or_else(|| {
                    anyhow::anyhow!("state.json 里 component:{name} 的 dep:{dep_name} 不存在")
                })?;
                for f in feats {
                    if !dep.features.contains_key(f) {
                        bail!("state.json 里 dep:{} 没有 feature {f}", dep.name);
                    }
                }
            }
            for feature in &state.features {
                if !component.features.contains_key(feature) {
                    let available: Vec<&str> =
                        component.features.keys().map(String::as_str).collect();
                    let hint = if available.is_empty() {
                        "无".to_owned()
                    } else {
                        available.join(", ")
                    };
                    bail!("state.json 里 component:{name} 没有 feature {feature}（可用：{hint}）");
                }
                if let Err(err) = crate::recipe::feature_available(reg, component, feature) {
                    bail!("state.json 里 {err}；改 state 或换依赖线后重试");
                }
            }
        }
        crate::targets::validate_selection(reg, &self.targets, "state.json")?;
        for (name, dep) in &self.deps {
            let target = reg
                .get(&format!("dep:{name}"))
                .ok_or_else(|| anyhow::anyhow!("state.json 里的 dep:{name} 不存在"))?;
            for f in &dep.features {
                if !target.features.contains_key(f) {
                    bail!("state.json 里 dep:{name} 没有 feature {f}");
                }
                if let Err(err) = crate::recipe::feature_available(reg, target, f) {
                    bail!("state.json 里 {err}；装上缺少的命令，或修改 state.json");
                }
            }
        }
        Ok(())
    }

    /// 当前构建方案 = 状态里的组件 ∪ 本次命令行点名的组件。
    pub fn scheme_with(&self, cli_names: &[String]) -> Vec<String> {
        let mut names: Vec<String> = if self.active {
            self.components.keys().cloned().collect()
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

    /// 把方案写进 product.components；求解器只认这一份，不从 recipe 取默认组件。
    pub fn apply_scheme(&self, reg: &mut Registry, names: &[String]) -> Result<()> {
        for name in names {
            reg.component(name)?;
        }
        let product_id = reg.product_any()?.id();
        reg.targets
            .get_mut(&product_id)
            .expect("product 目标刚刚检查过")
            .components = names.to_vec();
        Ok(())
    }

    /// 把状态里的 mode/deps 和命令行选择合并成一次求解用的选择。
    pub fn effective_selection(
        &self,
        reg: &Registry,
        cli: &Selection,
        presets: &Selection,
        names: &[String],
    ) -> Result<Selection> {
        let mut out = Selection::default();
        for name in names {
            let component = reg.component(name)?;
            let mut spec = cli.per_component.get(name).cloned().unwrap_or_default();
            if let Some(state) = self.components.get(name) {
                if spec.mode.is_none() && !state.mode.is_empty() {
                    spec.mode = Some(state.mode.clone());
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
            } else if let Some(preset) = presets.per_component.get(name) {
                // 首次选择这个组件：CLI 没写的 feature/mode 用 preset 补齐。
                let explicit_deps: BTreeSet<String> = spec.deps.keys().cloned().collect();
                if spec.mode.is_none() {
                    spec.mode = preset.mode.clone();
                }
                if spec.features.is_none() {
                    spec.features = preset.features.clone();
                }
                for (dep, feats) in &preset.deps {
                    if explicit_deps.contains(dep) {
                        continue;
                    }
                    let entry = spec.deps.entry(dep.clone()).or_default();
                    for f in feats {
                        if !entry.contains(f) {
                            entry.push(f.clone());
                        }
                    }
                }
            }
            // 没写 mode / feature 的组件在这里补上解析后的值：state 保存的是
            // 解析结果，标记里的方案命令和指纹也必须按同一份结果渲染，否则首装后
            // 重跑同一条 install 会算出不同的方案，被误判成“已装方案与本次不同”。
            if spec.mode.is_none() && !component.default_mode.is_empty() {
                spec.mode = Some(component.default_mode.clone());
            }
            if spec.features.is_none() {
                spec.features = Some(BTreeSet::new());
            }
            out.per_component.insert(name.clone(), spec);
        }
        Ok(out)
    }

    pub fn add_component_with_preset(
        &mut self,
        reg: &Registry,
        name: &str,
        preset: Option<&ComponentSpec>,
    ) -> Result<bool> {
        if self.components.contains_key(name) {
            return Ok(false);
        }
        let component = reg.component(name)?;
        let mode = match preset.and_then(|spec| spec.mode.clone()) {
            Some(mode) => mode,
            None => reg.default_mode(component)?,
        };
        let deps = preset.map(|spec| spec.deps.clone()).unwrap_or_default();
        let features = preset
            .and_then(|spec| spec.features.clone())
            .map(|features| features.into_iter().collect())
            .unwrap_or_default();
        self.components.insert(
            name.to_owned(),
            ComponentState {
                line: String::new(),
                version: String::new(),
                mode,
                installed: false,
                deps,
                features,
            },
        );
        self.active = true;
        Ok(true)
    }

    pub fn remove_component(&mut self, name: &str) -> bool {
        let Some(component) = self.components.remove(name) else {
            return false;
        };
        // bundled 组件（openssl/curl）没有自己的线，同名 binding 属于依赖库。
        if !component.line.is_empty() {
            self.bindings.remove(name);
        }
        true
    }

    pub fn set_installed_all(&mut self) {
        for component in self.components.values_mut() {
            component.installed = true;
        }
    }

    /// 把本次求解使用的 mode/deps 写回状态，保留 installed。
    pub fn sync_selection(
        &mut self,
        reg: &Registry,
        sel: &Selection,
        names: &[String],
    ) -> Result<()> {
        for name in names {
            let component = reg.component(name)?;
            let spec = sel.per_component.get(name).cloned().unwrap_or_default();
            let entry = self.components.entry(name.clone()).or_default();
            entry.mode = sel.mode_of(component);
            entry.line = component.line.clone();
            entry.version = component.version.clone();
            entry.deps = spec.deps;
            entry.features = spec
                .features
                .map(|set| set.into_iter().collect())
                .unwrap_or_default();
        }
        // 方案之外的组件不留在状态里。
        self.components.retain(|name, _| names.contains(name));
        self.active = true;
        Ok(())
    }

    /// 保存的组件方案转成 Selection，供 `scheme` 导出。
    pub fn as_selection(&self) -> Selection {
        Selection {
            per_component: self
                .components
                .iter()
                .map(|(name, entry)| {
                    (
                        name.clone(),
                        ComponentSpec {
                            mode: (!entry.mode.is_empty()).then(|| entry.mode.clone()),
                            features: Some(entry.features.iter().cloned().collect()),
                            deps: entry.deps.clone(),
                            version: None,
                        },
                    )
                })
                .collect(),
        }
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

    /// 用本次闭包重建 deps 状态；重编成功后才能覆盖 line / version。
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

    /// 记录本次实际解析到的工具（host + built），按名字 / 版本排序。
    pub fn set_tools(&mut self, resolution: &crate::tools::Resolution) {
        self.tools = resolution
            .report()
            .into_iter()
            .map(|tool| ToolState {
                name: tool.name.clone(),
                version: tool.version.clone(),
                provider: tool.provider.as_str().to_owned(),
            })
            .collect();
    }

    /// `--lock` 用：state 里记录过的 built 版本，按名字分组。
    pub fn locked_tools(&self) -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for tool in &self.tools {
            if tool.provider == "store" && !tool.version.is_empty() {
                out.entry(tool.name.clone())
                    .or_default()
                    .push(tool.version.clone());
            }
        }
        out
    }

    /// 不重编的 remove：保留已记录的 line / version，只收窄 feature、删掉闭包外的 dep。    ///
    /// 当前 catalog 解析出来的版本没有真正编译过，不能覆盖 state。
    pub fn prune_deps(&mut self, order: &[&Target], features: &BTreeMap<String, Vec<String>>) {
        let needed: BTreeSet<&str> = order
            .iter()
            .filter(|t| t.kind == Kind::Dep)
            .map(|t| t.name.as_str())
            .collect();
        self.deps.retain(|name, _| needed.contains(name.as_str()));
        for (name, dep) in self.deps.iter_mut() {
            dep.features = features.get(name).cloned().unwrap_or_default();
        }
    }

    /// 按最终闭包收窄显式线。
    ///
    /// 只在已经算出 `order` 的路径上调用（目前是 `remove`），避免在这里猜
    /// 哪些名字会被当前方案使用；闭包外的 binding 没有语义意义。
    pub fn prune_bindings(&mut self, desired: &BTreeSet<String>) {
        self.bindings.retain(|name, _| desired.contains(name));
    }

    pub fn component_status(&self, name: &str) -> &'static str {
        match self.components.get(name) {
            Some(c) if c.installed => "installed",
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
                    configure_options: Default::default(),
                    disable: vec![],
                    requires: Default::default(),
                },
            );
        }
        t
    }

    fn component(name: &str) -> Target {
        let mut t = Target {
            kind: Kind::Component,
            name: name.into(),
            product: "php".into(),
            source: "bundled".into(),
            modes: vec!["static".into(), "shared".into()],
            default_mode: "static".into(),
            modules: vec![name.into()],
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
            toolchain: None,
            bundled: vec![],
            bundled_disable: Default::default(),
            components: vec![],
            deps: Default::default(),
            profile: Default::default(),
            targets: Default::default(),
            selected_targets: Default::default(),
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
            feature_checks: Default::default(),
            features: Default::default(),
            ignored: Default::default(),
            configure: vec![],
            configure_options: Default::default(),
            make: vec![],
            artifacts: vec![],
            desc: String::new(),
            line: String::new(),
            series: String::new(),
        }
    }

    fn registry() -> Registry {
        let mut curl = component("curl");
        curl.needs = vec!["libcurl".into()];
        let mut swoole = component("swoole");
        swoole.needs = vec!["libcurl".into()];
        swoole.source = "external".into();
        swoole.modes = vec!["shared".into()];
        swoole.default_mode = "shared".into();
        let mut profile = crate::recipe::Profile::default();
        profile.modes.insert(
            "static".into(),
            crate::recipe::ProfileMode {
                strategy: "fold".into(),
                ..Default::default()
            },
        );
        profile.modes.insert(
            "shared".into(),
            crate::recipe::ProfileMode {
                strategy: "standalone".into(),
                ..Default::default()
            },
        );
        profile.defaults.insert("bundled".into(), "static".into());
        profile.defaults.insert("external".into(), "shared".into());
        let product = Target {
            kind: Kind::Product,
            name: "php".into(),
            version: "8.5.12".into(),
            url: "https://example.com/php.tar.gz".into(),
            series: "8.5".into(),
            bundled: vec!["curl".into()],
            profile,
            ..blank()
        };
        Registry::from_targets(vec![
            dep("libcurl", &["ssl", "nghttp2"]),
            curl,
            swoole,
            product,
        ])
        .unwrap()
    }

    #[test]
    fn targets_round_trip_through_json() {
        let targets =
            crate::targets::parse_cli(&["cli,fpm=user:www-data,group:www-data".to_owned()])
                .unwrap();
        let state = ProjectState {
            targets: targets.clone(),
            ..ProjectState::default()
        };
        let text = serde_json::to_string(&state).unwrap();
        assert!(text.contains("\"targets\""), "{text}");
        let back: ProjectState = serde_json::from_str(&text).unwrap();
        assert_eq!(back.targets, state.targets);
    }

    #[test]
    fn default_scheme_is_empty() {
        let state = ProjectState::default();
        assert!(state.scheme_with(&[]).is_empty());

        let mut reg = registry();
        state.apply_scheme(&mut reg, &[]).unwrap();
        assert!(reg.product_any().unwrap().components.is_empty());
    }

    #[test]
    fn scheme_takes_cli_components() {
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
        state.add_component_with_preset(&reg, "curl", None).unwrap();
        state
            .add_component_with_preset(&reg, "swoole", None)
            .unwrap();
        state
            .components
            .get_mut("curl")
            .unwrap()
            .deps
            .insert("libcurl".into(), vec!["nghttp2".into()]);
        let cli = Selection::default();
        let names = state.scheme_with(&[]);
        let sel = state
            .effective_selection(&reg, &cli, &Selection::default(), &names)
            .unwrap();
        assert_eq!(names, vec!["curl", "swoole"]);
        assert_eq!(sel.features(&reg)["libcurl"], vec!["nghttp2"]);
        assert_eq!(sel.mode_of(reg.component("swoole").unwrap()), "shared");
    }

    #[test]
    fn first_install_scheme_survives_state_sync() {
        let reg = registry();
        let names = vec!["curl".to_owned(), "swoole".to_owned()];
        let empty = Selection::default();
        let targets = TargetSelection::new();
        let digest =
            |sel: &Selection| crate::marker::scheme_digest(sel, &BTreeMap::new(), &targets);

        // 首次安装：默认组件从 bundle 铺开，mode 没有显式写。
        let first = ProjectState::default()
            .effective_selection(&reg, &empty, &empty, &names)
            .unwrap();
        assert_eq!(first.per_component["curl"].mode.as_deref(), Some("static"));
        assert_eq!(
            first.per_component["swoole"].mode.as_deref(),
            Some("shared")
        );

        // 装完 sync_selection 把解析后的 mode 写进 state；再算一次方案指纹必须不变。
        let mut state = ProjectState::default();
        state.sync_selection(&reg, &first, &names).unwrap();
        let again = state
            .effective_selection(&reg, &empty, &empty, &names)
            .unwrap();
        assert_eq!(digest(&first), digest(&again));
    }

    #[test]
    fn preset_applies_only_without_existing_state() {
        let reg = registry();
        let mut presets = Selection::default();
        presets.per_component.insert(
            "curl".into(),
            ComponentSpec {
                mode: Some("shared".into()),
                features: Some(BTreeSet::from(["ssl".into()])),
                deps: BTreeMap::from([("libcurl".into(), vec!["ssl".into()])]),
                ..Default::default()
            },
        );
        let names = vec!["curl".to_owned()];

        let state = ProjectState::default();
        let sel = state
            .effective_selection(&reg, &Selection::default(), &presets, &names)
            .unwrap();
        assert_eq!(
            sel.features_of(reg.component("curl").unwrap()),
            Some(&BTreeSet::from(["ssl".to_owned()]))
        );
        assert_eq!(sel.deps_of(reg.component("curl").unwrap()).count(), 1);
        assert_eq!(sel.mode_of(reg.component("curl").unwrap()), "shared");

        let mut state = ProjectState::default();
        state
            .add_component_with_preset(&reg, "curl", presets.per_component.get("curl"))
            .unwrap();
        assert_eq!(state.components["curl"].features, vec!["ssl"]);
        assert_eq!(state.components["curl"].deps["libcurl"], vec!["ssl"]);

        // 已有 state（即使 feature 为空）优先，不再套 preset。
        state.components.get_mut("curl").unwrap().features.clear();
        let sel = state
            .effective_selection(&reg, &Selection::default(), &presets, &names)
            .unwrap();
        assert_eq!(
            sel.features_of(reg.component("curl").unwrap()),
            Some(&BTreeSet::new())
        );
    }

    #[test]
    fn prune_deps_keeps_versions_and_drops_deps_outside_the_closure() {
        let reg = registry();
        let mut state = ProjectState::default();
        state.deps.insert(
            "libcurl".into(),
            DepState {
                line: "8.23".into(),
                version: "8.23.0".into(),
                features: vec!["ssl".into(), "nghttp2".into()],
            },
        );
        // 已删除组件留下的历史依赖，这次闭包里没有。
        state.deps.insert(
            "openssl".into(),
            DepState {
                line: "3.5".into(),
                version: "3.5.9".into(),
                features: vec![],
            },
        );

        let order = vec![reg.get("dep:libcurl").unwrap()];
        state.prune_deps(
            &order,
            &BTreeMap::from([("libcurl".to_string(), vec!["ssl".to_string()])]),
        );

        assert!(!state.deps.contains_key("openssl"));
        // 版本是上次真正编译过的，不按当前 catalog 重算。
        assert_eq!(state.deps["libcurl"].line, "8.23");
        assert_eq!(state.deps["libcurl"].version, "8.23.0");
        assert_eq!(state.deps["libcurl"].features, vec!["ssl"]);
    }

    #[test]
    fn prune_bindings_keeps_only_the_current_closure() {
        let mut state = ProjectState::default();
        state.bindings.insert("openssl".into(), "3.5".into());
        state.bindings.insert("libmemcached".into(), "1.1".into());
        state.bindings.insert("swoole".into(), "6.2".into());

        state.prune_bindings(&BTreeSet::from([
            "php".to_owned(),
            "openssl".to_owned(),
            "swoole".to_owned(),
        ]));

        assert_eq!(state.bindings.len(), 2);
        assert_eq!(state.bindings["openssl"], "3.5");
        assert_eq!(state.bindings["swoole"], "6.2");
        assert!(!state.bindings.contains_key("libmemcached"));
    }

    #[test]
    fn status_reflects_installed_flag() {
        let reg = registry();
        let mut state = ProjectState::default();
        assert_eq!(state.component_status("curl"), "available");
        state.add_component_with_preset(&reg, "curl", None).unwrap();
        assert_eq!(state.component_status("curl"), "pending");
        state.set_installed_all();
        assert_eq!(state.component_status("curl"), "installed");
        state.remove_component("curl");
        assert_eq!(state.component_status("curl"), "available");
    }

    #[test]
    fn old_state_version_is_rejected() {
        let path = std::env::temp_dir().join(format!("phprt-state-v2-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"version":2,"php":"8.5.12","ext":{}}"#).unwrap();
        let err = ProjectState::load(&path).unwrap_err().to_string();
        assert!(err.contains("旧格式"), "{err}");
        let _ = std::fs::remove_file(&path);
    }
}
