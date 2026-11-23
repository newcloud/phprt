//! 镜像家族：按镜像内容识别发行版，套用必需的构建环境设置。
//!
//! 家族只描述“这类镜像需要什么”，不靠镜像引用或 digest 判断。识别依据是
//! 镜像里的包管理器和 `/etc/os-release` 的 ID / ID_LIKE；引用可以换镜像站，
//! tag 和 digest 会漂，不能作为判断条件。
//!
//! 配置写在 `recipes/images.toml` 的 `[families.<名字>]`：
//!
//! ```toml
//! [families.debian]
//! pm = "apt"
//! id_like = ["debian", "ubuntu"]
//! reason = "..."
//! apt_sandbox = "root"
//! ```
//!
//! 有设置时必须写 `reason`，让配置本身能解释为什么要改镜像里的文件。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::host_commands::parse_os_release;

/// rootless 容器里给 apt 用的配置文件，位于实例 rootfs 内。
pub const APT_SANDBOX_FILE: &str = "etc/apt/apt.conf.d/99phprt-rootless-sandbox";
const APT_SANDBOX_CONTENT: &str = "APT::Sandbox::User \"root\";\n";

/// 一个镜像家族。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Family {
    pub name: String,
    /// 包管理器，只能是 `apt` 或 `dnf`。
    pub pm: String,
    /// 匹配 `/etc/os-release` 的 `ID` / `ID_LIKE`；空表示只按包管理器匹配。
    pub id_like: Vec<String>,
    /// 为什么需要这些设置；有设置时必填。
    pub reason: String,
    /// apt 沙箱用户；目前只支持 `root`。
    pub apt_sandbox: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFamily {
    pm: String,
    #[serde(default)]
    id_like: Vec<String>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    apt_sandbox: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FamiliesFile {
    #[serde(default)]
    families: BTreeMap<String, RawFamily>,
}

/// 从镜像内容得到的事实，用来匹配家族。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub pm: String,
    pub ids: BTreeSet<String>,
}

impl Facts {
    /// 读镜像 rootfs：包管理器 + `/etc/os-release` 的 ID / ID_LIKE。
    pub fn detect(rootfs: &Path) -> Result<Self> {
        let pm = phprt_buildimages::detect_pm(rootfs)?.to_string();
        let text = fs::read_to_string(rootfs.join("etc/os-release"))
            .or_else(|_| fs::read_to_string(rootfs.join("usr/lib/os-release")))
            .unwrap_or_default();
        let fields = parse_os_release(&text);
        let mut ids = BTreeSet::new();
        for key in ["ID", "ID_LIKE"] {
            if let Some(value) = fields.get(key) {
                for token in value.split_whitespace() {
                    ids.insert(token.to_ascii_lowercase());
                }
            }
        }
        Ok(Self { pm, ids })
    }
}

/// 读 `recipes/images.toml` 里的 `[families.*]`，并做严格校验。
pub fn load(path: &Path) -> Result<BTreeMap<String, Family>> {
    let text = fs::read_to_string(path).with_context(|| format!("读不到 {}", path.display()))?;
    let file: FamiliesFile =
        toml::from_str(&text).with_context(|| format!("解析 {}", path.display()))?;
    let mut out = BTreeMap::new();
    for (name, raw) in file.families {
        let family = validate(name, raw)?;
        out.insert(family.name.clone(), family);
    }
    Ok(out)
}

fn validate(name: String, raw: RawFamily) -> Result<Family> {
    if name.trim().is_empty() {
        bail!("镜像家族名字不能为空");
    }
    if raw.pm != "apt" && raw.pm != "dnf" {
        bail!("镜像家族 {name}: pm 只能是 apt 或 dnf，写的是 {:?}", raw.pm);
    }
    let mut id_like: Vec<String> = Vec::new();
    for item in raw.id_like {
        let item = item.trim().to_ascii_lowercase();
        if item.is_empty() {
            bail!("镜像家族 {name}: id_like 里有空项");
        }
        if id_like.contains(&item) {
            bail!("镜像家族 {name}: id_like 重复 {item}");
        }
        id_like.push(item);
    }
    let apt_sandbox = match raw.apt_sandbox {
        None => None,
        Some(value) if value == "root" => Some(value),
        Some(value) => {
            bail!("镜像家族 {name}: 不支持的 apt_sandbox = {value:?}，目前只支持 \"root\"")
        }
    };
    let reason = raw.reason.trim().to_string();
    if apt_sandbox.is_some() && reason.is_empty() {
        bail!("镜像家族 {name}: 有设置时必须写 reason，说明为什么需要它");
    }
    Ok(Family {
        name,
        pm: raw.pm,
        id_like,
        reason,
        apt_sandbox,
    })
}

/// 按内容匹配家族：包管理器相同，且 `id_like` 为空或与镜像 ID / ID_LIKE 有交集。
///
/// 同时命中多个家族是配置错误，直接报错，不按名字顺序挑一个。
pub fn matched<'a>(
    families: &'a BTreeMap<String, Family>,
    facts: &Facts,
) -> Result<Option<&'a Family>> {
    let hits: Vec<&Family> = families
        .values()
        .filter(|family| family.pm == facts.pm)
        .filter(|family| {
            family.id_like.is_empty() || family.id_like.iter().any(|id| facts.ids.contains(id))
        })
        .collect();
    match hits.len() {
        0 => Ok(None),
        1 => Ok(Some(hits[0])),
        _ => {
            let names: Vec<&str> = hits.iter().map(|family| family.name.as_str()).collect();
            bail!(
                "镜像同时匹配多个家族：{}；请检查 recipes/images.toml 的 [families.*]",
                names.join(", ")
            )
        }
    }
}

/// 读镜像内容并匹配家族。
pub fn detect<'a>(
    families: &'a BTreeMap<String, Family>,
    rootfs: &Path,
) -> Result<Option<&'a Family>> {
    let facts = Facts::detect(rootfs)?;
    matched(families, &facts)
}

/// 列表用的家族名；读不出镜像内容时显示 `-`，配置歧义仍然报错。
pub fn name_for(families: &BTreeMap<String, Family>, rootfs: &Path) -> Result<String> {
    let facts = match Facts::detect(rootfs) {
        Ok(facts) => facts,
        Err(_) => return Ok("-".to_string()),
    };
    Ok(matched(families, &facts)?
        .map(|family| family.name.clone())
        .unwrap_or_else(|| "-".to_string()))
}

/// 把家族设置写进实例 rootfs。重复调用结果相同。
pub fn apply(family: &Family, rootfs: &Path) -> Result<()> {
    if family.apt_sandbox.as_deref() != Some("root") {
        return Ok(());
    }
    let path = rootfs.join(APT_SANDBOX_FILE);
    if fs::read_to_string(&path).ok().as_deref() == Some(APT_SANDBOX_CONTENT) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("创建 {}", parent.display()))?;
    }
    fs::write(&path, APT_SANDBOX_CONTENT).with_context(|| format!("写 {}", path.display()))?;
    Ok(())
}

/// 家族生效的设置项，用于输出。
pub fn settings(family: &Family) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(value) = &family.apt_sandbox {
        out.push(format!("apt_sandbox={value}"));
    }
    out
}

/// 一行摘要：`debian（apt_sandbox=root）`。
pub fn summary(family: &Family) -> String {
    let settings = settings(family);
    if settings.is_empty() {
        family.name.clone()
    } else {
        format!("{}（{}）", family.name, settings.join(", "))
    }
}

/// `buildimages add` / `use` 用的多行说明。
pub fn print_details(family: &Family) {
    println!("家族      {}", family.name);
    let settings = settings(family);
    if !settings.is_empty() {
        println!("设置      {}", settings.join(", "));
    }
    let mut lines = family.reason.lines();
    if let Some(first) = lines.next().filter(|line| !line.trim().is_empty()) {
        println!("原因      {first}");
        for line in lines {
            println!("          {line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "phprt-family-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_families(dir: &Path, text: &str) -> PathBuf {
        let file = dir.join("images.toml");
        fs::write(&file, text).unwrap();
        file
    }

    #[test]
    fn load_parses_family() {
        let dir = temp("load");
        let file = write_families(
            &dir,
            r#"
[families.debian]
pm = "apt"
id_like = ["Debian", "ubuntu"]
reason = """
rootless 用户命名空间只映射 uid 0，
apt 的 _apt 用户无法 setuid。
"""
apt_sandbox = "root"
"#,
        );
        let families = load(&file).unwrap();
        let family = &families["debian"];
        assert_eq!(family.pm, "apt");
        assert_eq!(family.id_like, vec!["debian", "ubuntu"]);
        assert_eq!(family.apt_sandbox.as_deref(), Some("root"));
        assert!(family.reason.contains("uid 0"));
        assert_eq!(summary(family), "debian（apt_sandbox=root）");
    }

    #[test]
    fn load_rejects_unknown_field() {
        let dir = temp("unknown-field");
        let file = write_families(
            &dir,
            r#"
[families.debian]
pm = "apt"
bootstrap = ["true"]
"#,
        );
        let err = format!("{:#}", load(&file).unwrap_err());
        assert!(err.contains("unknown field"), "{err}");
    }

    #[test]
    fn load_rejects_unknown_pm() {
        let dir = temp("unknown-pm");
        let file = write_families(
            &dir,
            r#"
[families.debian]
pm = "yum"
"#,
        );
        let err = load(&file).unwrap_err().to_string();
        assert!(err.contains("pm 只能是 apt 或 dnf"), "{err}");
    }

    #[test]
    fn load_rejects_empty_id_like() {
        let dir = temp("empty-id");
        let file = write_families(
            &dir,
            r#"
[families.debian]
pm = "apt"
id_like = [""]
"#,
        );
        let err = load(&file).unwrap_err().to_string();
        assert!(err.contains("id_like 里有空项"), "{err}");
    }

    #[test]
    fn load_requires_reason_for_settings() {
        let dir = temp("reason");
        let file = write_families(
            &dir,
            r#"
[families.debian]
pm = "apt"
apt_sandbox = "root"
"#,
        );
        let err = load(&file).unwrap_err().to_string();
        assert!(err.contains("必须写 reason"), "{err}");
    }

    #[test]
    fn load_rejects_unknown_setting_value() {
        let dir = temp("setting");
        let file = write_families(
            &dir,
            r#"
[families.debian]
pm = "apt"
reason = "x"
apt_sandbox = "nobody"
"#,
        );
        let err = load(&file).unwrap_err().to_string();
        assert!(err.contains("不支持的 apt_sandbox"), "{err}");
    }

    fn families(text: &str) -> BTreeMap<String, Family> {
        let dir = temp("match");
        let file = write_families(&dir, text);
        load(&file).unwrap()
    }

    fn facts(pm: &str, ids: &[&str]) -> Facts {
        Facts {
            pm: pm.to_string(),
            ids: ids.iter().map(|id| id.to_string()).collect(),
        }
    }

    #[test]
    fn matched_uses_pm_and_os_release_ids() {
        let map = families(
            r#"
[families.debian]
pm = "apt"
id_like = ["debian", "ubuntu"]
"#,
        );
        assert_eq!(
            matched(&map, &facts("apt", &["debian"]))
                .unwrap()
                .unwrap()
                .name,
            "debian"
        );
        assert_eq!(
            matched(&map, &facts("apt", &["ubuntu"]))
                .unwrap()
                .unwrap()
                .name,
            "debian"
        );
        assert!(matched(&map, &facts("apt", &["fedora"])).unwrap().is_none());
        assert!(matched(&map, &facts("dnf", &["debian"])).unwrap().is_none());
    }

    #[test]
    fn empty_id_like_matches_whole_pm() {
        let map = families(
            r#"
[families.apt]
pm = "apt"
"#,
        );
        assert_eq!(
            matched(&map, &facts("apt", &["alpine"]))
                .unwrap()
                .unwrap()
                .name,
            "apt"
        );
    }

    #[test]
    fn multiple_matches_are_an_error() {
        let map = families(
            r#"
[families.debian]
pm = "apt"
id_like = ["debian"]

[families.linux]
pm = "apt"
id_like = ["linux"]
"#,
        );
        let err = matched(&map, &facts("apt", &["debian", "linux"]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("同时匹配多个家族"), "{err}");
        assert!(err.contains("debian"), "{err}");
        assert!(err.contains("linux"), "{err}");
    }

    #[test]
    fn apply_writes_apt_conf_idempotently() {
        let dir = temp("apply");
        let rootfs = dir.join("rootfs");
        fs::create_dir_all(rootfs.join("etc/apt")).unwrap();
        let family = Family {
            name: "debian".to_string(),
            pm: "apt".to_string(),
            id_like: vec!["debian".to_string()],
            reason: "test".to_string(),
            apt_sandbox: Some("root".to_string()),
        };
        apply(&family, &rootfs).unwrap();
        let path = rootfs.join(APT_SANDBOX_FILE);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "APT::Sandbox::User \"root\";\n"
        );
        apply(&family, &rootfs).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "APT::Sandbox::User \"root\";\n"
        );
    }

    #[test]
    fn apply_leaves_other_families_untouched() {
        let dir = temp("apply-none");
        let rootfs = dir.join("rootfs");
        fs::create_dir_all(&rootfs).unwrap();
        let family = Family {
            name: "fedora".to_string(),
            pm: "dnf".to_string(),
            id_like: vec!["fedora".to_string()],
            reason: String::new(),
            apt_sandbox: None,
        };
        apply(&family, &rootfs).unwrap();
        assert!(!rootfs.join(APT_SANDBOX_FILE).exists());
    }
}
