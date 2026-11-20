//! 工具链：探测宿主编译器、按产品声明选档、给出该档的编译参数。
//!
//! 只做探测和参数合并，不下载 gcc、不启动容器。glibc / ld / binutils 不声明。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::recipe::{Target, ToolchainRange};
use crate::tools;

/// 实际编译器环境。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Facts {
    pub cc: String,
    pub gcc: String,
    pub cxx: String,
    pub gxx: String,
    pub triple: String,
    pub arch: String,
}

/// 选中的档位：facts + 这次该追加的编译参数。
#[derive(Debug, Clone)]
pub struct Resolved {
    pub facts: Facts,
    pub cflags: String,
    pub cxxflags: String,
    /// 档位的可读描述，如 `>= 14`，用于 plan / marker。
    pub gcc_band: String,
    pub gxx_band: String,
}

/// 本次编译器检查结果：解析成功的档位，或缺失的命令名。
#[derive(Debug, Clone, Default)]
pub struct Checked {
    pub resolved: Option<Resolved>,
    /// 不在 PATH 的编译器（实际命令名，如 `gcc` / `g++`）。
    pub missing: Vec<String>,
}

/// 构建实际使用的编译器：产品 profile 的 CC / CXX 优先，其次进程环境，最后 gcc / g++。
pub fn cc_name(product: &Target) -> String {
    env_or(product, "CC", "gcc")
}

pub fn cxx_name(product: &Target) -> String {
    env_or(product, "CXX", "g++")
}

fn env_or(product: &Target, key: &str, fallback: &str) -> String {
    product
        .profile
        .env
        .get(key)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var(key)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| fallback.to_owned())
}

/// 探测编译器版本、triple 和架构；C++ 只有声明了 gxx 才探测。
/// 命令不存在或输出解析不出时直接报错，不猜版本。
pub fn probe(product: &Target, with_cxx: bool) -> Result<Facts> {
    let cc = cc_name(product);
    let gcc = tools::probe_program(&cc, &["--version"])
        .with_context(|| format!("探测编译器 {cc} 失败"))?;
    let triple = tools::run_program(&cc, &["-dumpmachine"])
        .with_context(|| format!("探测编译器 {cc} 的 triple 失败"))?
        .trim()
        .to_owned();
    let arch = arch();

    let (cxx, gxx) = if with_cxx {
        let cxx = cxx_name(product);
        let gxx = tools::probe_program(&cxx, &["--version"])
            .with_context(|| format!("探测编译器 {cxx} 失败"))?;
        (cxx, gxx)
    } else {
        (String::new(), String::new())
    };

    Ok(Facts {
        cc,
        gcc,
        cxx,
        gxx,
        triple,
        arch,
    })
}

/// 产品声明了 toolchain 就选档；没声明返回 None（不检查、不加参数）。
pub fn resolve(product: &Target) -> Result<Option<Resolved>> {
    let Some(toolchain) = &product.toolchain else {
        return Ok(None);
    };
    let facts = probe(product, !toolchain.gxx.is_empty())?;

    let (gcc_band, cflags) = if toolchain.gcc.is_empty() {
        (String::new(), String::new())
    } else {
        let band = pick(&toolchain.gcc, &facts.gcc, "gcc", &product.name)?;
        (describe(band), band.cflags.trim().to_owned())
    };
    let (gxx_band, cxxflags) = if toolchain.gxx.is_empty() {
        (String::new(), String::new())
    } else {
        let band = pick(&toolchain.gxx, &facts.gxx, "gxx", &product.name)?;
        (describe(band), band.cxxflags.trim().to_owned())
    };

    Ok(Some(Resolved {
        facts,
        cflags,
        cxxflags,
        gcc_band,
        gxx_band,
    }))
}

/// 产品声明了 toolchain 就检查；命令缺失时不探测，返回缺失清单。
///
/// 命令存在但版本不在任何档位里仍然报错：装包不一定能解决，不降级成提示。
pub fn check(product: &Target) -> Result<Checked> {
    let missing = missing_commands(product);
    if !missing.is_empty() {
        return Ok(Checked {
            resolved: None,
            missing,
        });
    }
    Ok(Checked {
        resolved: resolve(product)?,
        missing: Vec::new(),
    })
}

/// 只检查命令是否存在，不探测版本；返回实际命令名（`CC` / `CXX`）。
pub fn missing_commands(product: &Target) -> Vec<String> {
    let Some(toolchain) = &product.toolchain else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let cc = cc_name(product);
    if !tools::on_path(&cc) {
        out.push(cc);
    }
    if !toolchain.gxx.is_empty() {
        let cxx = cxx_name(product);
        if !out.contains(&cxx) && !tools::on_path(&cxx) {
            out.push(cxx);
        }
    }
    out
}

/// 记录用的架构：`uname -m` 优先，取不到时用编译期常量。
pub fn arch() -> String {
    tools::run_program("uname", &["-m"])
        .map(|text| text.trim().to_owned())
        .ok()
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| std::env::consts::ARCH.to_owned())
}

fn pick<'a>(
    ranges: &'a [ToolchainRange],
    version: &str,
    item: &str,
    product: &str,
) -> Result<&'a ToolchainRange> {
    ranges
        .iter()
        .find(|range| crate::version::in_range(version, &range.min, &range.max))
        .ok_or_else(|| {
            let declared = ranges.iter().map(describe).collect::<Vec<_>>().join("、");
            anyhow::anyhow!(
                "产品 {product} 声明 {item} 档位 {declared}，实际 {item} {version} 不在任何一档里；\
                 不下载、不安装，请换编译器或补档位"
            )
        })
}

fn describe(range: &ToolchainRange) -> String {
    match (range.min.is_empty(), range.max.is_empty()) {
        (false, false) => format!(">= {} <= {}", range.min, range.max),
        (false, true) => format!(">= {}", range.min),
        (true, false) => format!("<= {}", range.max),
        (true, true) => "任意版本".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(min: &str, max: &str, cflags: &str) -> ToolchainRange {
        ToolchainRange {
            min: min.to_owned(),
            max: max.to_owned(),
            cflags: cflags.to_owned(),
            cxxflags: String::new(),
        }
    }

    #[test]
    fn bands_use_the_written_segments() {
        let ranges = vec![range("9", "13", ""), range("14", "", "-std=gnu17")];
        assert_eq!(pick(&ranges, "13.9.9", "gcc", "php").unwrap().cflags, "");
        assert_eq!(
            pick(&ranges, "15.3.1", "gcc", "php").unwrap().cflags,
            "-std=gnu17"
        );
        assert!(pick(&ranges, "8.9", "gcc", "php").is_err());
    }

    #[test]
    fn band_description_is_stable() {
        assert_eq!(describe(&range("9", "13", "")), ">= 9 <= 13");
        assert_eq!(describe(&range("14", "", "")), ">= 14");
        assert_eq!(describe(&range("", "8", "")), "<= 8");
    }
}
