//! 每个产品系列都必须能加载 recipe 并求出方案。
//!
//! bundle 指向不存在的线、组件 needs 不存在的依赖、产品 LINES 漏系列这类错误
//! 只有真正加载时才暴露；这里把 `plan` 跑一遍，让 `cargo test` 默认覆盖。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn recipes() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("recipes")
}

fn product_series() -> Vec<String> {
    let dir = recipes().join("bundles");
    let mut out = Vec::new();
    for product in fs::read_dir(&dir).expect("读不到 recipes/bundles") {
        let product = product.expect("读 bundles 条目").path();
        if !product.is_dir() {
            continue;
        }
        let name = product
            .file_name()
            .expect("bundles 子目录有名字")
            .to_string_lossy()
            .into_owned();
        for bundle in fs::read_dir(&product).expect("读产品 bundles") {
            let bundle = bundle.expect("读 bundle 条目").path();
            if bundle.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let series = bundle
                .file_stem()
                .expect("bundle 有文件名")
                .to_string_lossy()
                .into_owned();
            out.push(format!("{name}@{series}"));
        }
    }
    out.sort();
    out
}

#[test]
fn every_product_series_plans() {
    let root = std::env::temp_dir().join(format!("phprt-recipe-smoke-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let all = product_series();
    assert!(!all.is_empty(), "recipes/bundles 下没有产品系列");
    for target in all {
        let out = Command::new(env!("CARGO_BIN_EXE_phprt"))
            .arg("--root")
            .arg(&root)
            .arg("--recipes")
            .arg(recipes())
            .arg("plan")
            .arg(&target)
            .output()
            .expect("启动 phprt plan");
        assert!(
            out.status.success(),
            "{target} plan 失败：\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let _ = fs::remove_dir_all(&root);
}
