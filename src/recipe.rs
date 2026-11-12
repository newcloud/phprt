use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use starlark::environment::{Globals, Module};
use starlark::eval::Evaluator;
use starlark::syntax::{AstModule, Dialect};

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
}

/// 扩展自己的一个子配置。开关是扩展的，方括号里的 feature 转给 dep。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubDef {
    #[serde(default)]
    pub dep: String,
    #[serde(default)]
    pub configure: Vec<String>,
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
    #[serde(default)]
    pub exts: Vec<String>,
    #[serde(default)]
    pub modules: Vec<String>,
    #[serde(default = "default_origin")]
    pub origin: String,
    #[serde(default)]
    pub link: Vec<String>,
    #[serde(default)]
    pub subs: BTreeMap<String, SubDef>,
    #[serde(default)]
    pub shared_from: String,
    #[serde(default)]
    pub ini: String,
    #[serde(default)]
    pub ini_lines: Vec<String>,
    #[serde(default)]
    pub features: BTreeMap<String, Feature>,
    #[serde(default)]
    pub configure: Vec<String>,
    #[serde(default)]
    pub make: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub desc: String,
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
                for (sub, def) in &self.subs {
                    if sub.is_empty() {
                        bail!("{at}: ext:{} 有空名字的子配置", self.name);
                    }
                    if !def.dep.is_empty() && !self.needs.iter().any(|n| n == &def.dep) {
                        bail!(
                            "{at}: ext:{} 的子配置 {sub} 指向 dep:{}，但它不在 needs 里",
                            self.name,
                            def.dep
                        );
                    }
                }
            }
            Kind::Php => {
                if self.version.is_empty() {
                    bail!("{at}: php:{} 缺少 version", self.name);
                }
                if self.url.is_empty() {
                    bail!("{at}: php:{} 缺少 url", self.name);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExtSpec {
    pub link: Option<Mode>,
    /// 子配置名 -> 转给 dep 的 feature 列表
    pub subs: BTreeMap<String, Vec<String>>,
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
                if part.starts_with("link:") || part.starts_with("sub:") {
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
                        } else if let Some(rest) = item.strip_prefix("sub:") {
                            let (sub, features) = parse_sub(reg, ext, rest)?;
                            if spec.subs.insert(sub.clone(), features).is_some() {
                                bail!("--ext {name} 里重复指定了子配置 {sub}");
                            }
                        } else {
                            bail!("--ext {name} 里未知选项 {item}（只支持 link: / sub:）");
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

    pub fn subs_of<'a>(
        &'a self,
        ext: &Target,
    ) -> impl Iterator<Item = (&'a String, &'a Vec<String>)> + 'a {
        self.per_ext
            .get(&ext.name)
            .map(|s| s.subs.iter())
            .into_iter()
            .flatten()
    }

    /// 本次选择给各 dep 带来的 feature 并集。
    pub fn features(&self, reg: &Registry) -> BTreeMap<String, Vec<String>> {
        let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for ext in reg.of_kind(Kind::Ext) {
            for (sub, feats) in self.subs_of(ext) {
                let Some(def) = ext.subs.get(sub) else {
                    continue;
                };
                if def.dep.is_empty() {
                    continue;
                }
                if feats.is_empty() {
                    continue;
                }
                let entry = out.entry(def.dep.clone()).or_default();
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

fn parse_sub(reg: &Registry, ext: &Target, rest: &str) -> Result<(String, Vec<String>)> {
    let (name, list) = match rest.find('[') {
        Some(i) => {
            if !rest.ends_with(']') {
                bail!("ext:{} 的子配置写法不完整：sub:{rest}", ext.name);
            }
            (&rest[..i], Some(&rest[i + 1..rest.len() - 1]))
        }
        None => (rest, None),
    };
    if name.is_empty() {
        bail!("ext:{} 的 sub: 后面缺少名字", ext.name);
    }
    let def = ext
        .subs
        .get(name)
        .ok_or_else(|| anyhow!("ext:{} 契约里没有子配置 {name}", ext.name))?;

    let mut features = Vec::new();
    if let Some(list) = list {
        if def.dep.is_empty() {
            bail!(
                "ext:{} 的子配置 {name} 没有绑定依赖，不能带方括号",
                ext.name
            );
        }
        let dep = reg.get(&format!("dep:{}", def.dep)).ok_or_else(|| {
            anyhow!(
                "ext:{} 的子配置 {name} 指向不存在的 dep:{}",
                ext.name,
                def.dep
            )
        })?;
        for f in list.split([',', '+']) {
            let f = f.trim();
            if f.is_empty() {
                bail!("ext:{} 的子配置 {name} 的 feature 列表里有空项", ext.name);
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
            bail!("当前原型只支持一个 php recipe");
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
            for (sub, def) in &t.subs {
                if def.dep.is_empty() {
                    continue;
                }
                let id = format!("dep:{}", def.dep);
                if !self.targets.contains_key(&id) {
                    bail!("{} 的子配置 {sub} 指向不存在的依赖 {id}", t.id());
                }
            }
        }
        Ok(())
    }

    pub fn load(dir: &Path) -> Result<Self> {
        let defs_path = dir.join("defs.star");
        let defs = fs::read_to_string(&defs_path)
            .with_context(|| format!("读不到 {}", defs_path.display()))?;

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

        let globals = Globals::standard();
        let dialect = Dialect::Standard;
        let mut targets = Vec::new();
        for path in files {
            let src =
                fs::read_to_string(&path).with_context(|| format!("读不到 {}", path.display()))?;
            let json = eval_file(&path, &defs, &src, &globals, &dialect)?;
            let target: Target = serde_json::from_value(json)
                .with_context(|| format!("{}: TARGET 字段不合法", path.display()))?;
            target.validate(&path)?;
            targets.push(target);
        }
        Self::from_targets(targets)
    }
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

fn eval_file(
    path: &Path,
    defs: &str,
    src: &str,
    globals: &Globals,
    dialect: &Dialect,
) -> Result<serde_json::Value> {
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
        let value = module
            .get("TARGET")
            .ok_or_else(|| anyhow!("{name} 没有定义顶层 TARGET"))?;
        value
            .to_json_value()
            .with_context(|| format!("{name}: TARGET 无法转成 JSON"))
    })
}
