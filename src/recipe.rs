use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use starlark::environment::{Globals, Module};
use starlark::eval::Evaluator;
use starlark::syntax::{AstModule, Dialect};

use crate::catalog::Catalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dep,
    Ext,
    Php,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Dep => "dep",
            Kind::Ext => "ext",
            Kind::Php => "php",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Static,
    Shared,
}

impl Mode {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "static" => Ok(Mode::Static),
            "shared" => Ok(Mode::Shared),
            other => bail!("未知链接方式 {other}，只支持 static 或 shared"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Static => "static",
            Mode::Shared => "shared",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Feature {
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub configure: Vec<String>,
    /// feature 未启用时应用的参数（关闭上游自动探测）。
    #[serde(default)]
    pub disable: Vec<String>,
    /// ext feature 的可用条件；dep feature 不允许写。
    #[serde(default)]
    pub requires: Requires,
}

/// ext feature 的可用条件：允许的 PHP 系列、允许的依赖线。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Requires {
    #[serde(default)]
    pub php: Vec<String>,
    #[serde(default)]
    pub dep: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub kind: Kind,
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub needs: Vec<String>,
    /// 构建期工具依赖（dep）；参与构建顺序和 PATH，不参与链接。
    #[serde(default)]
    pub tools: Vec<String>,
    /// PHP 版本事实：该版本内置扩展列表（只有 php target 使用）。
    #[serde(default)]
    pub builtin_exts: Vec<String>,
    /// PHP 版本事实：内置扩展 -> 不选中时使用的关闭参数（只有 php target 使用）。
    #[serde(default)]
    pub builtin_disable: BTreeMap<String, String>,
    #[serde(default)]
    pub exts: Vec<String>,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default = "default_origin")]
    pub origin: String,
    #[serde(default)]
    pub link: Vec<String>,
    /// 依赖集成：依赖名 -> 选中该依赖时额外加给扩展的 configure 参数。
    /// 出现即表示允许 dep:NAME；key 必须是 needs 里的依赖。
    #[serde(default)]
    pub dep_configure: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub shared_from: String,
    #[serde(default)]
    pub ini: String,
    #[serde(default)]
    pub ini_lines: Vec<String>,
    #[serde(default)]
    pub features: BTreeMap<String, Feature>,
    /// 看过但决定不暴露的上游选项 -> 理由；只有 dep / ext 使用。
    #[serde(default)]
    pub ignored: BTreeMap<String, String>,
    #[serde(default)]
    pub configure: Vec<String>,
    #[serde(default)]
    pub make: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub desc: String,
    /// 选中的版本线（6.2 / 3.5 / 8.5）。
    #[serde(default)]
    pub line: String,
    /// PHP recipe 的系列（8.5）；其他目标为空。
    #[serde(default)]
    pub series: String,
}

fn default_origin() -> String {
    "builtin".to_owned()
}

impl Target {
    pub fn id(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.name)
    }

    /// builtin 默认静态，PECL 默认动态。
    pub fn default_mode(&self) -> Result<Mode> {
        match self.origin.as_str() {
            "builtin" => Ok(Mode::Static),
            "pecl" => Ok(Mode::Shared),
            other => bail!(
                "{} 的 origin={other} 不合法，只能是 builtin 或 pecl",
                self.id()
            ),
        }
    }

    fn validate(&self, path: &Path) -> Result<()> {
        let at = path.display();
        if self.name.is_empty() {
            bail!("{at}: name 不能为空");
        }
        match self.kind {
            Kind::Dep => {
                if self.version.is_empty() {
                    bail!("{at}: dep:{} 缺少 version", self.name);
                }
                if self.url.is_empty() {
                    bail!("{at}: dep:{} 缺少 url", self.name);
                }
                if !self.exts.is_empty() {
                    bail!("{at}: dep:{} 不允许 exts 字段", self.name);
                }
                if !self.tools.is_empty() {
                    bail!("{at}: dep:{} 不允许 tools 字段", self.name);
                }
                if !self.builtin_exts.is_empty() || !self.builtin_disable.is_empty() {
                    bail!(
                        "{at}: dep:{} 不允许 builtin_exts/builtin_disable",
                        self.name
                    );
                }
                if !self.dep_configure.is_empty() {
                    bail!("{at}: dep:{} 不允许 dep_configure", self.name);
                }
                for (name, f) in &self.features {
                    if !f.requires.php.is_empty() || !f.requires.dep.is_empty() {
                        bail!("{at}: dep:{} 的 feature {name} 不允许 requires", self.name);
                    }
                }
                self.check_ignored(&at)?;
            }
            Kind::Ext => {
                if self.modules.is_empty() {
                    bail!("{at}: ext:{} 缺少 modules", self.name);
                }
                if self.origin != "builtin" && self.origin != "pecl" {
                    bail!(
                        "{at}: ext:{} 的 origin={} 不合法，只能是 builtin 或 pecl",
                        self.name,
                        self.origin
                    );
                }
                if self.link.is_empty() {
                    bail!("{at}: ext:{} 缺少 link 契约", self.name);
                }
                for m in &self.link {
                    if m != "static" && m != "shared" {
                        bail!(
                            "{at}: ext:{} 的 link 值 {m} 不合法，只能是 static 或 shared",
                            self.name
                        );
                    }
                }
                let default = self.default_mode()?;
                if !self.link.iter().any(|m| m == default.as_str()) {
                    bail!(
                        "{at}: ext:{} 是 {}，默认必须支持 {}，但 link={}",
                        self.name,
                        self.origin,
                        default.as_str(),
                        self.link.join(",")
                    );
                }
                if self.link.iter().any(|m| m == "shared") {
                    if self.ini.is_empty() {
                        bail!("{at}: ext:{} 支持 shared，但缺少 ini", self.name);
                    }
                    match self.origin.as_str() {
                        "builtin" => {
                            if self.shared_from.is_empty() {
                                bail!(
                                    "{at}: ext:{} 是 builtin shared，但缺少 shared_from",
                                    self.name
                                );
                            }
                        }
                        "pecl" if self.url.is_empty() || self.version.is_empty() => {
                            bail!(
                                "{at}: ext:{} 是 pecl shared，但缺少 url 或 version",
                                self.name
                            );
                        }
                        _ => {}
                    }
                }
                for tool in &self.tools {
                    if tool.is_empty() {
                        bail!("{at}: ext:{} 的 tools 里有空名字", self.name);
                    }
                }
                if !self.builtin_exts.is_empty() || !self.builtin_disable.is_empty() {
                    bail!(
                        "{at}: ext:{} 不允许 builtin_exts/builtin_disable",
                        self.name
                    );
                }
                for (dep, flags) in &self.dep_configure {
                    if dep.is_empty() {
                        bail!("{at}: ext:{} 的 dep_configure 里有空名字", self.name);
                    }
                    if !self.needs.iter().any(|n| n == dep) {
                        bail!(
                            "{at}: ext:{} 的 dep_configure 里有 dep:{dep}，但它不在 needs 里；\
                             声明必需依赖用 needs",
                            self.name
                        );
                    }
                    for flag in flags {
                        if flag.is_empty() {
                            bail!("{at}: ext:{} 的 dep_configure[{dep}] 里有空参数", self.name);
                        }
                    }
                }
                for (name, f) in &self.features {
                    if name.is_empty() {
                        bail!("{at}: ext:{} 的 features 里有空名字", self.name);
                    }
                    if !f.disable.is_empty() {
                        bail!(
                            "{at}: ext:{} 的 feature {name} 只允许 needs/configure/requires；\
                             disable 只用于 dep 的 feature",
                            self.name
                        );
                    }
                    for need in &f.needs {
                        if need.is_empty() {
                            bail!(
                                "{at}: ext:{} 的 feature {name} 的 needs 里有空名字",
                                self.name
                            );
                        }
                    }
                    for flag in &f.configure {
                        if flag.is_empty() {
                            bail!("{at}: ext:{} 的 feature {name} 里有空参数", self.name);
                        }
                    }
                    for php in &f.requires.php {
                        if php.is_empty() {
                            bail!(
                                "{at}: ext:{} 的 feature {name} 的 requires.php 里有空名字",
                                self.name
                            );
                        }
                    }
                    for (dep, lines) in &f.requires.dep {
                        if !self.needs.iter().any(|n| n == dep) {
                            bail!(
                                "{at}: ext:{} 的 feature {name} 的 requires.dep 里有 {dep}，\
                                 但它不在 needs 里",
                                self.name
                            );
                        }
                        if lines.is_empty() {
                            bail!(
                                "{at}: ext:{} 的 feature {name} 的 requires.dep[{dep}] 是空的",
                                self.name
                            );
                        }
                        for line in lines {
                            if line.is_empty() {
                                bail!(
                                    "{at}: ext:{} 的 feature {name} 的 requires.dep[{dep}] 里有空线",
                                    self.name
                                );
                            }
                        }
                    }
                }
            }
            Kind::Php => {
                if self.builtin_exts.is_empty() {
                    bail!("{at}: php:{} 的 builtin_exts 不能为空", self.name);
                }
                let mut seen = BTreeSet::new();
                for name in &self.builtin_exts {
                    if name.is_empty() {
                        bail!("{at}: php:{} 的 builtin_exts 里有空名字", self.name);
                    }
                    if !seen.insert(name.clone()) {
                        bail!("{at}: php:{} 的 builtin_exts 重复: {name}", self.name);
                    }
                }
                for (name, flag) in &self.builtin_disable {
                    if !seen.contains(name) {
                        bail!(
                            "{at}: php:{} 的 builtin_disable 里有非内置扩展 {name}",
                            self.name
                        );
                    }
                    if flag.contains(char::is_whitespace)
                        || !(flag.starts_with("--without-") || flag.starts_with("--disable-"))
                    {
                        bail!(
                            "{at}: php:{} 的 builtin_disable[{name}] 必须是单个 --without-* 或 --disable-* 参数",
                            self.name
                        );
                    }
                }
                if self.series.is_empty() {
                    bail!("{at}: php:{} 缺少 series", self.name);
                }
                if !self.dep_configure.is_empty()
                    || !self.features.is_empty()
                    || !self.ignored.is_empty()
                {
                    bail!(
                        "{at}: php:{} 不允许 dep_configure/features/ignored",
                        self.name
                    );
                }
                if self.version.is_empty() {
                    bail!("{at}: php:{} 缺少 version", self.name);
                }
                if self.url.is_empty() {
                    bail!("{at}: php:{} 缺少 url", self.name);
                }
            }
        }
        self.check_ignored(&at)?;
        Ok(())
    }

    fn check_ignored(&self, at: &std::path::Display<'_>) -> Result<()> {
        for (option, reason) in &self.ignored {
            if !option.starts_with("--") {
                bail!(
                    "{at}: {}:{} 的 ignored key {option} 必须是 --enable-* / --with-* 形式的选项",
                    self.kind.as_str(),
                    self.name
                );
            }
            if reason.trim().is_empty() {
                bail!(
                    "{at}: {}:{} 的 ignored[{option}] 需要写理由",
                    self.kind.as_str(),
                    self.name
                );
            }
        }
        Ok(())
    }
}

/// recipe 声明的一条版本线：支持哪些 PHP，以及依赖哪些线。
#[derive(Debug, Clone, Default)]
pub struct LineMeta {
    pub php: Vec<String>,
    pub deps: BTreeMap<String, Vec<String>>,
}

/// 第一阶段加载出来的 recipe 元数据，不需要构建上下文。
#[derive(Debug, Clone)]
pub struct RecipeMeta {
    pub kind: Kind,
    pub name: String,
    pub series: Option<String>,
    pub lines: BTreeMap<String, LineMeta>,
    /// PHP 系列 -> 该扩展允许的依赖线（builtin 扩展用，如 ext:openssl）。
    pub php_lines: BTreeMap<String, Vec<String>>,
    /// php recipe 的内置扩展事实；其他 recipe 为空。
    pub builtin_exts: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ExtSpec {
    pub link: Option<Mode>,
    /// 依赖名 -> 转给该依赖的 feature 列表
    pub deps: BTreeMap<String, Vec<String>>,
    /// `feature:[...]` 选中的扩展自己的 feature；None 表示没写，
    /// Some(空) 表示显式关闭（用于关掉 bundle 默认值）。
    pub features: Option<BTreeSet<String>>,
    /// `version:6.2` 指定的版本线；只在这里记录，绑定在加载 recipe 前完成。
    pub version: Option<String>,
}

/// 命令行 `--ext` 里与版本线有关的最小信息，用于加载 recipe 之前解析绑定。
#[derive(Debug, Clone, Default)]
pub struct RawExt {
    pub name: String,
    pub version: Option<String>,
}

/// 只解析 `--ext` 的扩展名和 `version:`，不依赖 recipe。
pub fn preparse(specs: &[String]) -> Result<Vec<RawExt>> {
    let mut out: Vec<RawExt> = Vec::new();
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();

    for spec in specs {
        let mut current: Option<usize> = None;
        for raw in split_top(spec, ',')? {
            let part = raw.trim();
            if part.is_empty() {
                bail!("--ext {spec} 里有空的片段");
            }
            if let Some(version) = part.strip_prefix("version:") {
                let idx =
                    current.ok_or_else(|| anyhow!("--ext {spec} 的第一个片段不能是 {part}"))?;
                if version.is_empty() {
                    bail!("--ext {spec} 的 version: 后面缺少版本线");
                }
                if out[idx].version.replace(version.to_owned()).is_some() {
                    bail!("--ext 里重复指定了 {} 的版本线", out[idx].name);
                }
                continue;
            }
            if part.starts_with("link:") || part.starts_with("dep:") || part.starts_with("feature:")
            {
                current.ok_or_else(|| anyhow!("--ext {spec} 的第一个片段不能是 {part}"))?;
                continue;
            }

            let (name, value) = match part.find(['=', ':']) {
                Some(i) => (&part[..i], Some(&part[i + 1..])),
                None => (part, None),
            };
            if name.is_empty() {
                bail!("--ext {part} 缺少扩展名");
            }
            if seen.insert(name.to_owned(), ()).is_some() {
                bail!("--ext 里重复指定了扩展 {name}");
            }
            let mut ext = RawExt {
                name: name.to_owned(),
                version: None,
            };
            if let Some(value) = value {
                if value.is_empty() {
                    bail!("--ext {name} 的选项是空的");
                }
                if let Some(version) = value.strip_prefix("version:") {
                    if version.is_empty() {
                        bail!("--ext {name} 的 version: 后面缺少版本线");
                    }
                    ext.version = Some(version.to_owned());
                }
            }
            current = Some(out.len());
            out.push(ext);
        }
    }
    Ok(out)
}

/// 命令行 `--ext` 的选择结果。
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub per_ext: BTreeMap<String, ExtSpec>,
}

impl Selection {
    pub fn parse(reg: &Registry, specs: &[String]) -> Result<Self> {
        let mut sel = Selection::default();

        for spec in specs {
            // 逗号既分隔扩展，也分隔同一个扩展的选项；[] 里的逗号不参与切分。
            let mut chunks: Vec<String> = Vec::new();
            for raw in split_top(spec, ',')? {
                let part = raw.trim();
                if part.is_empty() {
                    bail!("--ext {spec} 里有空的片段");
                }
                if part.starts_with("link:")
                    || part.starts_with("dep:")
                    || part.starts_with("feature:")
                    || part.starts_with("version:")
                {
                    let last = chunks
                        .last_mut()
                        .ok_or_else(|| anyhow!("--ext {spec} 的第一个片段不能是 {part}"))?;
                    last.push(',');
                    last.push_str(part);
                } else {
                    chunks.push(part.to_owned());
                }
            }

            for chunk in chunks {
                let (name, value) = match chunk.find(['=', ':']) {
                    Some(i) => (&chunk[..i], Some(&chunk[i + 1..])),
                    None => (chunk.as_str(), None),
                };
                if name.is_empty() {
                    bail!("--ext {chunk} 缺少扩展名");
                }
                let ext = reg.ext(name)?;
                if sel.per_ext.contains_key(name) {
                    bail!("--ext 里重复指定了扩展 {name}");
                }

                let mut spec = ExtSpec::default();
                if let Some(value) = value {
                    if value.is_empty() {
                        bail!("--ext {name} 的选项是空的");
                    }
                    for item in split_top(value, ',')? {
                        let item = item.trim();
                        if let Some(m) = item.strip_prefix("link:") {
                            spec.link = Some(Mode::parse(m)?);
                        } else if let Some(rest) = item.strip_prefix("dep:") {
                            let (dep, features) = parse_dep(reg, ext, rest)?;
                            if spec.deps.insert(dep.clone(), features).is_some() {
                                bail!("--ext {name} 里重复指定了 dep:{dep}");
                            }
                        } else if let Some(rest) = item.strip_prefix("feature:") {
                            let feats = parse_feature(reg, ext, rest)?;
                            let set = spec.features.get_or_insert_with(BTreeSet::new);
                            for f in feats {
                                set.insert(f);
                            }
                        } else if let Some(version) = item.strip_prefix("version:") {
                            if version.is_empty() {
                                bail!("--ext {name} 的 version: 后面缺少版本线");
                            }
                            if spec.version.replace(version.to_owned()).is_some() {
                                bail!("--ext {name} 里重复指定了 version:");
                            }
                        } else {
                            bail!(
                                "--ext {name} 里未知选项 {item}\
                                 （只支持 link: / dep: / feature: / version:）"
                            );
                        }
                    }
                }

                if let Some(link) = spec.link
                    && !ext.link.iter().any(|m| m == link.as_str())
                {
                    bail!(
                        "ext:{name} 契约不支持 {}（支持：{}）",
                        link.as_str(),
                        ext.link.join(",")
                    );
                }
                sel.per_ext.insert(name.to_owned(), spec);
            }
        }
        Ok(sel)
    }

    pub fn mode_of(&self, ext: &Target) -> Result<Mode> {
        match self.per_ext.get(&ext.name).and_then(|s| s.link) {
            Some(m) => Ok(m),
            None => ext.default_mode(),
        }
    }

    pub fn deps_of<'a>(
        &'a self,
        ext: &Target,
    ) -> impl Iterator<Item = (&'a String, &'a Vec<String>)> + 'a {
        self.per_ext
            .get(&ext.name)
            .map(|s| s.deps.iter())
            .into_iter()
            .flatten()
    }

    /// 选中的扩展 feature；None 表示没写，Some(空) 表示显式关闭。
    pub fn features_of(&self, ext: &Target) -> Option<&BTreeSet<String>> {
        self.per_ext
            .get(&ext.name)
            .and_then(|s| s.features.as_ref())
    }

    /// 选中的扩展 feature 额外需要的依赖；没选中就返回空。
    pub fn feature_needs<'a>(&'a self, ext: &'a Target) -> Vec<&'a String> {
        let Some(selected) = self.features_of(ext) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for name in selected {
            if let Some(def) = ext.features.get(name) {
                out.extend(def.needs.iter());
            }
        }
        out
    }

    /// 本次选择给各 dep 带来的 feature 并集。
    pub fn features(&self, reg: &Registry) -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for ext in reg.of_kind(Kind::Ext) {
            for (dep, feats) in self.deps_of(ext) {
                if !ext.dep_configure.contains_key(dep) || feats.is_empty() {
                    continue;
                }
                let entry = out.entry(dep.clone()).or_default();
                for f in feats {
                    if !entry.contains(f) {
                        entry.push(f.clone());
                    }
                }
            }
        }
        for feats in out.values_mut() {
            feats.sort();
        }
        out
    }
}

/// `dep:NAME` 或 `dep:NAME[f1,f2]`；名字是依赖名，方括号里的 feature 转给该依赖。
fn parse_dep(reg: &Registry, ext: &Target, rest: &str) -> Result<(String, Vec<String>)> {
    let (name, list) = match rest.find('[') {
        Some(i) => {
            if !rest.ends_with(']') {
                bail!("ext:{} 的 dep: 写法不完整：dep:{rest}", ext.name);
            }
            (&rest[..i], Some(&rest[i + 1..rest.len() - 1]))
        }
        None => (rest, None),
    };
    if name.is_empty() {
        bail!("ext:{} 的 dep: 后面缺少依赖名", ext.name);
    }
    if !ext.dep_configure.contains_key(name) {
        let available: Vec<&str> = ext.dep_configure.keys().map(String::as_str).collect();
        let hint = if available.is_empty() {
            "无".to_owned()
        } else {
            available.join(", ")
        };
        bail!(
            "ext:{} 没有依赖集成 {name}（可写：{hint}）；\
             需要在 recipe 的 dep_configure 里声明",
            ext.name
        );
    }
    let dep = reg
        .get(&format!("dep:{name}"))
        .ok_or_else(|| anyhow!("ext:{} 的 dep:{name} 指向不存在的依赖", ext.name))?;

    let mut features = Vec::new();
    if let Some(list) = list {
        for f in list.split([',', '+']) {
            let f = f.trim();
            if f.is_empty() {
                bail!("ext:{} 的 dep:{name} 的 feature 列表里有空项", ext.name);
            }
            if !dep.features.contains_key(f) {
                bail!("dep:{} 契约没有 feature {f}", dep.name);
            }
            if !features.iter().any(|x| x == f) {
                features.push(f.to_owned());
            }
        }
    }
    Ok((name.to_owned(), features))
}

/// `feature:[f1,f2]`；必须带方括号，空列表表示显式关闭全部 feature。
fn parse_feature(reg: &Registry, ext: &Target, rest: &str) -> Result<Vec<String>> {
    let list = match rest.strip_prefix('[') {
        Some(inner) => inner
            .strip_suffix(']')
            .ok_or_else(|| anyhow!("ext:{} 的 feature: 写法不完整：feature:{rest}", ext.name))?,
        None => bail!(
            "ext:{} 的 feature: 后面必须写方括号，例如 feature:[argon2]",
            ext.name
        ),
    };

    let mut features = Vec::new();
    if list.trim().is_empty() {
        return Ok(features);
    }
    for f in list.split(',') {
        let f = f.trim();
        if f.is_empty() {
            bail!("ext:{} 的 feature 列表里有空项", ext.name);
        }
        if !ext.features.contains_key(f) {
            let available: Vec<&str> = ext.features.keys().map(String::as_str).collect();
            let hint = if available.is_empty() {
                "无".to_owned()
            } else {
                available.join(", ")
            };
            bail!("ext:{} 没有 feature {f}（可用：{hint}）", ext.name);
        }
        feature_available(reg, ext, f)?;
        if !features.iter().any(|x| x == f) {
            features.push(f.to_owned());
        }
    }
    Ok(features)
}

/// 检查 ext 的一个 feature 在当前 PHP 系列和依赖线下是否可用。
pub(crate) fn feature_available(reg: &Registry, ext: &Target, name: &str) -> Result<()> {
    let Some(def) = ext.features.get(name) else {
        return Ok(());
    };
    let req = &def.requires;
    if req.php.is_empty() && req.dep.is_empty() {
        return Ok(());
    }

    let php = reg.php()?;
    if !req.php.is_empty() && !req.php.iter().any(|s| s == &php.series) {
        bail!(
            "ext:{} 的 feature {name} 需要 PHP {}，当前 PHP {}",
            ext.name,
            req.php.join(", "),
            php.series
        );
    }
    for (dep_name, lines) in &req.dep {
        let dep = reg.get(&format!("dep:{dep_name}")).ok_or_else(|| {
            anyhow!(
                "ext:{} 的 feature {name} 需要不存在的 dep:{dep_name}",
                ext.name
            )
        })?;
        if !lines.iter().any(|l| l == &dep.line) {
            let current = if dep.line.is_empty() {
                "未绑定".to_owned()
            } else {
                dep.line.clone()
            };
            bail!(
                "ext:{} 的 feature {name} 需要 dep:{dep_name} {}，当前 {current}",
                ext.name,
                lines.join(", ")
            );
        }
    }
    Ok(())
}

fn split_top(s: &str, sep: char) -> Result<Vec<&str>> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                if depth == 0 {
                    bail!("{s} 里有多余的 ]");
                }
                depth -= 1;
            }
            _ if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    if depth != 0 {
        bail!("{s} 里缺少 ]");
    }
    parts.push(&s[start..]);
    Ok(parts)
}

#[derive(Debug, Default)]
pub struct Registry {
    pub targets: BTreeMap<String, Target>,
}

impl Registry {
    pub fn from_targets(targets: impl IntoIterator<Item = Target>) -> Result<Self> {
        let mut map = BTreeMap::new();
        for t in targets {
            let id = t.id();
            if map.insert(id.clone(), t).is_some() {
                bail!("重复的目标 {id}");
            }
        }
        let reg = Self { targets: map };
        reg.validate_refs()?;
        Ok(reg)
    }

    pub fn get(&self, id: &str) -> Option<&Target> {
        self.targets.get(id)
    }

    pub fn of_kind(&self, kind: Kind) -> impl Iterator<Item = &Target> {
        self.targets.values().filter(move |t| t.kind == kind)
    }

    pub fn php(&self) -> Result<&Target> {
        let mut it = self.of_kind(Kind::Php);
        let first = it
            .next()
            .ok_or_else(|| anyhow!("recipes 里没有 php recipe"))?;
        if it.next().is_some() {
            bail!("当前一次只能加载一个 PHP 系列");
        }
        Ok(first)
    }

    pub fn ext(&self, name: &str) -> Result<&Target> {
        self.get(&format!("ext:{name}"))
            .ok_or_else(|| anyhow!("未知扩展 ext:{name}"))
    }

    pub fn resolve_name(&self, name: &str) -> Result<String> {
        if self.targets.contains_key(name) {
            return Ok(name.to_owned());
        }
        let matches: Vec<&String> = self
            .targets
            .keys()
            .filter(|id| id.split_once(':').map(|(_, n)| n) == Some(name))
            .collect();
        match matches.as_slice() {
            [] => bail!("未知目标 {name}"),
            [one] => Ok((*one).clone()),
            many => bail!(
                "目标名 {name} 有歧义，请写成 {}",
                many.iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" 或 ")
            ),
        }
    }

    fn validate_refs(&self) -> Result<()> {
        for t in self.targets.values() {
            for need in &t.needs {
                let id = format!("dep:{need}");
                if !self.targets.contains_key(&id) {
                    bail!("{} 的 needs 指向不存在的依赖 {id}", t.id());
                }
            }
            for tool in &t.tools {
                let id = format!("dep:{tool}");
                if !self.targets.contains_key(&id) {
                    bail!("{} 的 tools 指向不存在的依赖 {id}", t.id());
                }
            }
            for (name, f) in &t.features {
                for need in &f.needs {
                    let id = format!("dep:{need}");
                    if !self.targets.contains_key(&id) {
                        bail!("{} 的 feature {name} 指向不存在的依赖 {id}", t.id());
                    }
                }
            }
            for ext in &t.exts {
                let id = format!("ext:{ext}");
                if !self.targets.contains_key(&id) {
                    bail!("{} 的 exts 指向不存在的扩展 {id}", t.id());
                }
            }
            for dep in t.dep_configure.keys() {
                let id = format!("dep:{dep}");
                if !self.targets.contains_key(&id) {
                    bail!("{} 的 dep_configure 指向不存在的依赖 {id}", t.id());
                }
            }
        }
        Ok(())
    }
}

/// 第一阶段：只读 recipe 顶层数据，建立版本关系，不需要 ctx。
pub fn load_meta(dir: &Path, catalog: &Catalog) -> Result<Vec<RecipeMeta>> {
    let defs = read_defs(dir)?;
    let globals = Globals::standard();
    let dialect = Dialect::Standard;
    let mut metas = Vec::new();

    for path in recipe_files(dir)? {
        let (kind, name) = classify(&path)?;
        let src =
            fs::read_to_string(&path).with_context(|| format!("读不到 {}", path.display()))?;
        let raw = eval_meta(&path, &defs, &src, &globals, &dialect)?;
        let lines = match raw.lines_json {
            Some(value) => {
                parse_lines(value).with_context(|| format!("{}: LINES 不合法", path.display()))?
            }
            None => BTreeMap::new(),
        };
        let php_lines = match raw.php_lines_json {
            Some(value) => parse_php_lines(value)
                .with_context(|| format!("{}: PHP_LINES 不合法", path.display()))?,
            None => BTreeMap::new(),
        };
        if kind == Kind::Dep && (!lines.is_empty() || !php_lines.is_empty()) {
            bail!(
                "{}: dep recipe 不能声明 LINES/PHP_LINES（库与 PHP 无关）",
                path.display()
            );
        }

        let mut builtin_exts = Vec::new();
        let series = if kind == Kind::Php {
            if raw.is_function {
                bail!("{}: php recipe 的 TARGET 不能是函数", path.display());
            }
            let value = raw
                .target_json
                .ok_or_else(|| anyhow!("{}: 缺少 TARGET", path.display()))?;
            let obj = value
                .as_object()
                .ok_or_else(|| anyhow!("{}: TARGET 必须是 dict", path.display()))?;
            if obj.get("kind").and_then(|v| v.as_str()) != Some("php") {
                bail!("{}: 根目录的 recipe 必须是 php", path.display());
            }
            let series = obj
                .get("series")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("{}: php TARGET 缺少 series", path.display()))?
                .to_owned();
            if series != name {
                bail!(
                    "{}: 文件名 php{name}.star 与 series={series} 不一致",
                    path.display()
                );
            }
            builtin_exts = obj
                .get("builtin_exts")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .with_context(|| format!("{}: builtin_exts 不合法", path.display()))?
                .unwrap_or_default();
            Some(series)
        } else {
            None
        };

        metas.push(RecipeMeta {
            kind,
            name,
            series,
            lines,
            php_lines,
            builtin_exts,
        });
    }

    validate_meta_catalog(&metas, catalog)?;
    Ok(metas)
}

/// 第二阶段：按 ctx 构造具体 Target。
pub fn load_targets(
    dir: &Path,
    catalog: &Catalog,
    series: &str,
    metas: &[RecipeMeta],
    lines: &BTreeMap<String, String>,
    base_ctx: &serde_json::Value,
    builtin_exts: &[String],
) -> Result<Registry> {
    let defs = read_defs(dir)?;
    let globals = Globals::standard();
    let dialect = Dialect::Standard;
    let mut targets = Vec::new();

    for path in recipe_files(dir)? {
        let (kind, name) = classify(&path)?;
        if kind == Kind::Php && name != series {
            continue;
        }
        let src =
            fs::read_to_string(&path).with_context(|| format!("读不到 {}", path.display()))?;

        let is_builtin = kind == Kind::Ext && builtin_exts.iter().any(|item| item == &name);
        let mut ctx = base_ctx.clone();
        ctx["is_builtin"] = serde_json::Value::Bool(is_builtin);
        if kind != Kind::Php {
            let self_value = base_ctx
                .get("deps")
                .and_then(|deps| deps.get(&name))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            ctx["self"] = self_value;
        }

        let json = eval_target(&path, &defs, &src, &globals, &dialect, &ctx)?;
        let mut target: Target = serde_json::from_value(json)
            .with_context(|| format!("{}: TARGET 字段不合法", path.display()))?;

        match kind {
            Kind::Php => {
                if target.kind != Kind::Php {
                    bail!("{}: TARGET.kind 必须是 php", path.display());
                }
                let artifact = catalog
                    .get("php", series)
                    .ok_or_else(|| anyhow!("catalog 里没有 PHP {series}"))?;
                target.name = artifact.version.clone();
                target.version = artifact.version.clone();
                target.url = artifact.url.clone();
                target.sha256 = artifact.sha256.clone();
                target.series = series.to_owned();
                target.line = series.to_owned();
            }
            Kind::Dep | Kind::Ext => {
                if target.kind != kind {
                    bail!("{}: TARGET.kind 与目录不一致", path.display());
                }
                if target.name != name {
                    bail!(
                        "{}: TARGET.name={} 与文件名 {} 不一致",
                        path.display(),
                        target.name,
                        name
                    );
                }
                if kind == Kind::Ext {
                    target.origin = if is_builtin {
                        "builtin".to_owned()
                    } else {
                        "pecl".to_owned()
                    };
                }
                let versioned = metas
                    .iter()
                    .any(|m| m.kind == kind && m.name == name && !m.lines.is_empty());
                if versioned || kind == Kind::Dep {
                    target.line = lines.get(&name).cloned().unwrap_or_default();
                }
            }
        }

        target.validate(&path)?;
        targets.push(target);
    }

    if !targets.iter().any(|t| t.kind == Kind::Php) {
        bail!("没有找到 PHP {series} 的 recipe");
    }
    Registry::from_targets(targets)
}

fn read_defs(dir: &Path) -> Result<String> {
    let path = dir.join("defs.star");
    fs::read_to_string(&path).with_context(|| format!("读不到 {}", path.display()))
}

fn recipe_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for sub in ["dep", "ext"] {
        let d = dir.join(sub);
        if d.is_dir() {
            files.extend(star_files(&d)?);
        }
    }
    for f in star_files(dir)? {
        if f.file_name().and_then(|s| s.to_str()) != Some("defs.star") {
            files.push(f);
        }
    }
    if files.is_empty() {
        bail!("{} 下没有任何 recipe", dir.display());
    }
    files.sort();
    Ok(files)
}

fn star_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("读不到目录 {}", dir.display()))? {
        let path = entry?.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("star") {
            v.push(path);
        }
    }
    v.sort();
    Ok(v)
}

fn classify(path: &Path) -> Result<(Kind, String)> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("{}: 文件名不合法", path.display()))?;
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str());
    match parent {
        Some("dep") => Ok((Kind::Dep, stem.to_owned())),
        Some("ext") => Ok((Kind::Ext, stem.to_owned())),
        _ => {
            let series = stem
                .strip_prefix("php")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow!("{}: 根目录只允许 php<系列>.star", path.display()))?;
            Ok((Kind::Php, series.to_owned()))
        }
    }
}

struct MetaRaw {
    target_json: Option<serde_json::Value>,
    lines_json: Option<serde_json::Value>,
    php_lines_json: Option<serde_json::Value>,
    is_function: bool,
}

fn eval_meta(
    path: &Path,
    defs: &str,
    src: &str,
    globals: &Globals,
    dialect: &Dialect,
) -> Result<MetaRaw> {
    let name = path.display().to_string();
    let ast_defs = AstModule::parse("defs.star", defs.to_owned(), dialect)
        .map_err(|e| anyhow!("defs.star 解析失败: {e}"))?;
    let ast = AstModule::parse(&name, src.to_owned(), dialect)
        .map_err(|e| anyhow!("{name} 解析失败: {e}"))?;

    Module::with_temp_heap(|module| {
        let mut eval = Evaluator::new(&module);
        eval.eval_module(ast_defs, globals)
            .map_err(|e| anyhow!("defs.star 求值失败: {e}"))?;
        eval.eval_module(ast, globals)
            .map_err(|e| anyhow!("{name} 求值失败: {e}"))?;
        let target = module
            .get("TARGET")
            .ok_or_else(|| anyhow!("{name} 没有定义顶层 TARGET"))?;
        let is_function = target.get_type() == "function";
        let target_json = if is_function {
            None
        } else {
            Some(
                target
                    .to_json_value()
                    .with_context(|| format!("{name}: TARGET 无法转成 JSON"))?,
            )
        };
        let lines_json = module
            .get("LINES")
            .map(|value| value.to_json_value())
            .transpose()
            .with_context(|| format!("{name}: LINES 无法转成 JSON"))?;
        let php_lines_json = module
            .get("PHP_LINES")
            .map(|value| value.to_json_value())
            .transpose()
            .with_context(|| format!("{name}: PHP_LINES 无法转成 JSON"))?;
        Ok(MetaRaw {
            target_json,
            lines_json,
            php_lines_json,
            is_function,
        })
    })
}

fn eval_target(
    path: &Path,
    defs: &str,
    src: &str,
    globals: &Globals,
    dialect: &Dialect,
    ctx: &serde_json::Value,
) -> Result<serde_json::Value> {
    let name = path.display().to_string();
    let ast_defs = AstModule::parse("defs.star", defs.to_owned(), dialect)
        .map_err(|e| anyhow!("defs.star 解析失败: {e}"))?;
    let ast = AstModule::parse(&name, src.to_owned(), dialect)
        .map_err(|e| anyhow!("{name} 解析失败: {e}"))?;
    let ctx_src = format!("ctx = {}", starlark_literal(ctx));
    let ast_ctx = AstModule::parse("<ctx>", ctx_src, dialect)
        .map_err(|e| anyhow!("{name}: 注入 ctx 失败: {e}"))?;

    Module::with_temp_heap(|module| {
        let mut eval = Evaluator::new(&module);
        eval.eval_module(ast_defs, globals)
            .map_err(|e| anyhow!("defs.star 求值失败: {e}"))?;
        eval.eval_module(ast_ctx, globals)
            .map_err(|e| anyhow!("{name}: 注入 ctx 失败: {e}"))?;
        eval.eval_module(ast, globals)
            .map_err(|e| anyhow!("{name} 求值失败: {e}"))?;

        let target = module
            .get("TARGET")
            .ok_or_else(|| anyhow!("{name} 没有定义顶层 TARGET"))?;
        let value = if target.get_type() == "function" {
            let ctx_value = module
                .get("ctx")
                .ok_or_else(|| anyhow!("{name}: TARGET 是函数，但 ctx 不存在"))?;
            eval.eval_function(target, &[ctx_value], &[])
                .map_err(|e| anyhow!("{name}: 调用 target(ctx) 失败: {e}"))?
        } else {
            target
        };
        value
            .to_json_value()
            .with_context(|| format!("{name}: TARGET 无法转成 JSON"))
    })
}

fn parse_php_lines(value: serde_json::Value) -> Result<BTreeMap<String, Vec<String>>> {
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("PHP_LINES 必须是 dict"))?;
    let mut out = BTreeMap::new();
    for (php, value) in obj {
        let list = value
            .as_array()
            .ok_or_else(|| anyhow!("PHP_LINES.{php} 必须是数组"))?;
        let lines: Vec<String> = list
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow!("PHP_LINES.{php} 里必须是字符串"))
            })
            .collect::<Result<_>>()?;
        if lines.is_empty() {
            bail!("PHP_LINES.{php} 不能为空");
        }
        out.insert(php.clone(), lines);
    }
    Ok(out)
}

fn parse_lines(value: serde_json::Value) -> Result<BTreeMap<String, LineMeta>> {
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow!("LINES 必须是 dict"))?;
    let mut out = BTreeMap::new();
    for (line, value) in obj {
        let line_obj = value
            .as_object()
            .ok_or_else(|| anyhow!("LINES.{line} 必须是 dict"))?;
        let php = line_obj
            .get("php")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("LINES.{line} 缺少 php 数组"))?;
        let php: Vec<String> = php
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow!("LINES.{line}.php 必须是字符串"))
            })
            .collect::<Result<_>>()?;
        if php.is_empty() {
            bail!("LINES.{line}.php 不能为空");
        }
        let mut deps = BTreeMap::new();
        for (key, value) in line_obj {
            if key == "php" {
                continue;
            }
            let list = value
                .as_array()
                .ok_or_else(|| anyhow!("LINES.{line}.{key} 必须是数组"))?;
            let list: Vec<String> = list
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| anyhow!("LINES.{line}.{key} 里必须是字符串"))
                })
                .collect::<Result<_>>()?;
            deps.insert(key.clone(), list);
        }
        out.insert(line.clone(), LineMeta { php, deps });
    }
    Ok(out)
}

fn validate_meta_catalog(metas: &[RecipeMeta], catalog: &Catalog) -> Result<()> {
    let recipe_php: BTreeSet<String> = metas
        .iter()
        .filter(|m| m.kind == Kind::Php)
        .filter_map(|m| m.series.clone())
        .collect();
    let catalog_php: BTreeSet<String> = catalog
        .lines("php")
        .map(|l| l.keys().cloned().collect())
        .unwrap_or_default();
    if recipe_php != catalog_php {
        bail!(
            "catalog 的 PHP 系列与 recipe 不一致：recipe={:?} catalog={:?}",
            recipe_php,
            catalog_php
        );
    }

    for (name, lines) in &catalog.components {
        if name == "php" {
            continue;
        }
        let meta = metas
            .iter()
            .find(|m| m.kind == Kind::Dep && &m.name == name)
            .or_else(|| {
                metas
                    .iter()
                    .find(|m| m.kind == Kind::Ext && &m.name == name)
            })
            .ok_or_else(|| anyhow!("catalog 里的 {name} 没有 recipe"))?;
        if !meta.lines.is_empty() {
            let recipe_lines: BTreeSet<&String> = meta.lines.keys().collect();
            let catalog_lines: BTreeSet<&String> = lines.keys().collect();
            if recipe_lines != catalog_lines {
                bail!(
                    "{name} 的 LINES 与 catalog 不一致：recipe={:?} catalog={:?}",
                    recipe_lines,
                    catalog_lines
                );
            }
        }
    }

    for meta in metas.iter().filter(|m| m.kind != Kind::Php) {
        if !meta.lines.is_empty() && !catalog.components.contains_key(&meta.name) {
            bail!("{} 声明了 LINES，但 catalog 没有同名组件", meta.name);
        }
        if meta.php_lines.is_empty() {
            continue;
        }
        let Some(component) = catalog.lines(&meta.name) else {
            bail!("{} 声明了 PHP_LINES，但 catalog 没有同名组件", meta.name);
        };
        for (php, allowed) in &meta.php_lines {
            if !catalog_php.contains(php) {
                bail!("{} 的 PHP_LINES 里有未知 PHP {php}", meta.name);
            }
            if allowed.is_empty() {
                bail!("{} 的 PHP_LINES.{php} 不能为空", meta.name);
            }
            for line in allowed {
                if !component.contains_key(line) {
                    bail!(
                        "{} 的 PHP_LINES.{php} 引用了 catalog 里不存在的线 {line}",
                        meta.name
                    );
                }
            }
        }
        for php in &catalog_php {
            if !meta.php_lines.contains_key(php) {
                bail!("{} 的 PHP_LINES 缺少 PHP {php}", meta.name);
            }
        }
    }
    Ok(())
}

fn starlark_literal(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "None".to_owned(),
        serde_json::Value::Bool(true) => "True".to_owned(),
        serde_json::Value::Bool(false) => "False".to_owned(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => starlark_string(s),
        serde_json::Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(starlark_literal)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        serde_json::Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}: {}", starlark_string(k), starlark_literal(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn starlark_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bind;
    use crate::bundle::Bundle;

    fn dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("recipes")
    }

    fn load(
        series: &str,
        selected: &[&str],
        pins: &[(&str, &str)],
    ) -> Result<(Registry, bind::Bindings)> {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml"))?;
        let metas = load_meta(&dir, &catalog)?;
        let bundle = Bundle::load(&dir.join("bundles").join(format!("php{series}.toml")))?;
        let mut pin_map = BTreeMap::new();
        for (name, line) in pins {
            pin_map.insert((*name).to_owned(), (*line).to_owned());
        }
        let selected: Vec<String> = selected.iter().map(|s| (*s).to_owned()).collect();
        let bindings = bind::resolve(&catalog, &bundle, &metas, series, &selected, &pin_map)?;
        let php_version = catalog.php_version(series)?.to_owned();
        let ctx = bind::context(series, &php_version, &bindings);
        let builtin_exts = metas
            .iter()
            .find(|meta| meta.kind == Kind::Php && meta.series.as_deref() == Some(series))
            .map(|meta| meta.builtin_exts.clone())
            .unwrap_or_default();
        let reg = load_targets(
            &dir,
            &catalog,
            series,
            &metas,
            &bindings.lines,
            &ctx,
            &builtin_exts,
        )?;
        Ok((reg, bindings))
    }

    #[test]
    fn builtin_facts_come_from_the_php_recipe() {
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let php = reg.php().unwrap();
        assert!(php.builtin_exts.contains(&"openssl".to_owned()));
        assert!(php.builtin_exts.contains(&"libxml".to_owned()));
        assert_eq!(
            php.builtin_disable.get("libxml").map(String::as_str),
            Some("--without-libxml")
        );
        assert_eq!(reg.ext("openssl").unwrap().origin, "builtin");
        assert!(reg.ext("openssl").unwrap().url.is_empty());
        assert_eq!(reg.ext("swoole").unwrap().origin, "pecl");
        assert!(!reg.ext("swoole").unwrap().url.is_empty());
    }

    #[test]
    fn default_selection_comes_from_the_bundle_table() {
        let dir = dir();
        let bundle = Bundle::load(&dir.join("bundles/php8.5.toml")).unwrap();
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let sel = crate::bundle::selection(&reg, &bundle, "8.5").unwrap();
        assert_eq!(sel.mode_of(reg.ext("curl").unwrap()).unwrap(), Mode::Static);
        assert_eq!(sel.per_ext["curl"].deps["libcurl"], vec!["ssl".to_owned()]);
        assert_eq!(
            sel.features_of(reg.ext("openssl").unwrap()),
            Some(&BTreeSet::from(["argon2".to_owned()]))
        );
    }

    #[test]
    fn argon2_requires_the_right_php_and_openssl_line() {
        let (reg, _) = load("8.5", &["openssl"], &[]).unwrap();
        assert!(reg.ext("openssl").unwrap().features.contains_key("argon2"));
        assert!(Selection::parse(&reg, &["openssl=feature:[argon2]".into()]).is_ok());

        // openssl 线不满足：feature 仍然声明，但选择时报错。
        let (reg, _) = load("8.5", &["openssl"], &[("openssl", "3.0")]).unwrap();
        let err = Selection::parse(&reg, &["openssl=feature:[argon2]".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("需要 dep:openssl 3.5, 4.0，当前 3.0"), "{err}");

        // PHP 系列不满足。
        let (reg, _) = load("8.4", &["openssl"], &[]).unwrap();
        let err = Selection::parse(&reg, &["openssl=feature:[argon2]".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("需要 PHP 8.5, 8.6，当前 PHP 8.4"), "{err}");
    }

    #[test]
    fn feature_selection_is_validated_and_can_be_cleared() {
        let (reg, _) = load("8.5", &[], &[]).unwrap();
        let sel = Selection::parse(&reg, &["openssl=feature:[argon2]".into()]).unwrap();
        assert_eq!(
            sel.features_of(reg.ext("openssl").unwrap()),
            Some(&BTreeSet::from(["argon2".to_owned()]))
        );

        let sel = Selection::parse(&reg, &["openssl=feature:[]".into()]).unwrap();
        assert_eq!(
            sel.features_of(reg.ext("openssl").unwrap()),
            Some(&BTreeSet::from([]))
        );

        let err = Selection::parse(&reg, &["openssl=feature:argon2".into()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("必须写方括号"), "{err}");
    }

    #[test]
    fn every_series_builds_with_bundle_defaults() {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        for series in catalog.php_series() {
            let (reg, bindings) = load(&series, &["swoole", "openssl", "xdebug"], &[]).unwrap();
            let php = reg.php().unwrap();
            assert_eq!(php.version, catalog.php_version(&series).unwrap());
            assert_eq!(php.line, series);
            assert!(reg.get("dep:swoole").is_none());
            let swoole = reg.ext("swoole").unwrap();
            assert_eq!(swoole.line, bindings.lines["swoole"]);
            assert_eq!(swoole.version, bindings.artifacts["swoole"].version);
            assert!(
                swoole.needs.contains(&"openssl".to_owned()),
                "{series} swoole 应该依赖 openssl"
            );
            assert_eq!(
                reg.get("dep:openssl").unwrap().line,
                bindings.lines["openssl"]
            );
            assert_eq!(
                reg.get("dep:libcurl").unwrap().version,
                catalog
                    .get("libcurl", bindings.lines["libcurl"].as_str())
                    .unwrap()
                    .version
            );
        }
    }

    #[test]
    fn soft_default_switches_to_the_only_compatible_line() {
        let (_, bindings) = load("8.2", &["swoole"], &[("swoole", "4.8")]).unwrap();
        assert_eq!(bindings.lines["swoole"], "4.8");
        assert_eq!(bindings.lines["openssl"], "1.1.1");
        assert!(
            bindings
                .notes
                .iter()
                .any(|note| note.contains("自动改用 1.1.1")),
            "{:?}",
            bindings.notes
        );
    }

    #[test]
    fn hard_pin_conflict_is_an_error() {
        let err = load("8.2", &["swoole"], &[("swoole", "4.8"), ("openssl", "3.5")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("冲突"), "{err}");
    }

    #[test]
    fn unsupported_php_line_is_an_error() {
        let err = load("8.5", &["swoole"], &[("swoole", "4.8")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("不支持 PHP 8.5"), "{err}");
    }

    #[test]
    fn preparse_reads_version_option() {
        let raw =
            preparse(&["swoole=link:shared,dep:libcurl[ssl],version:6.2".to_owned()]).unwrap();
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].name, "swoole");
        assert_eq!(raw[0].version.as_deref(), Some("6.2"));

        let raw = preparse(&["curl,openssl=version:4.0".to_owned()]).unwrap();
        assert_eq!(raw.len(), 2);
        assert_eq!(raw[0].name, "curl");
        assert_eq!(raw[1].version.as_deref(), Some("4.0"));
    }

    #[test]
    fn ext_openssl_php_lines_are_enforced() {
        let (_, bindings) = load("7.4", &["openssl"], &[]).unwrap();
        assert_eq!(bindings.lines["openssl"], "1.1.1");

        let err = load("7.4", &["openssl"], &[("openssl", "3.5")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("冲突"), "{err}");

        let (_, bindings) = load("8.5", &["openssl"], &[("openssl", "4.0")]).unwrap();
        assert_eq!(bindings.lines["openssl"], "4.0");
    }

    #[test]
    fn dep_recipes_have_no_php_relationship() {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        let metas = load_meta(&dir, &catalog).unwrap();
        let dep = metas
            .iter()
            .find(|m| m.kind == Kind::Dep && m.name == "openssl")
            .unwrap();
        assert!(dep.lines.is_empty());
        assert!(dep.php_lines.is_empty());

        let ext = metas
            .iter()
            .find(|m| m.kind == Kind::Ext && m.name == "openssl")
            .unwrap();
        assert_eq!(ext.php_lines["8.5"], vec!["1.1.1", "3.0", "3.5", "4.0"]);
    }

    #[test]
    fn dep_recipe_cannot_declare_php_relations() {
        let dir = std::env::temp_dir().join(format!("phprt-dep-lines-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dep")).unwrap();
        std::fs::write(dir.join("defs.star"), "").unwrap();
        std::fs::write(
            dir.join("dep/foo.star"),
            r#"
LINES = {"1": {"php": ["8.5"]}}

def target(ctx):
    return {"kind": "dep", "name": "foo", "version": "1", "url": "https://example.com/foo.tar.gz"}

TARGET = target
"#,
        )
        .unwrap();
        let err = load_meta(&dir, &Catalog::default())
            .unwrap_err()
            .to_string();
        assert!(err.contains("dep recipe 不能声明"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn catalog_rejects_unknown_versions() {
        let dir = dir();
        let catalog = Catalog::load(&dir.join("catalog.toml")).unwrap();
        assert_eq!(catalog.find_php_series("8.5").unwrap(), "8.5");
        assert_eq!(catalog.find_php_series("php:8.5.12").unwrap(), "8.5");
        assert!(catalog.find_php_series("8.9").is_err());
    }
}
