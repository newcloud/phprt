//! `--lock` 的端到端测试：只跑 `plan` / `scheme`，不做下载和编译。
//!
//! 夹具是临时项目根 + 复制一份 recipes，改 catalog 模拟“catalog 升级但保留旧补丁”。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("phprt-plan-lock-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("recipes");
        copy_dir(&source, &root.join("recipes"));
        Self { root }
    }

    fn recipes(&self) -> PathBuf {
        self.root.join("recipes")
    }

    fn catalog(&self) -> PathBuf {
        self.recipes().join("catalog.toml")
    }

    fn write_state(&self, state: Value) {
        let dir = self.root.join(".phprt/php-8.5");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("state.json"),
            serde_json::to_string_pretty(&state).unwrap(),
        )
        .unwrap();
    }

    fn remove_state(&self) {
        let _ = fs::remove_dir_all(self.root.join(".phprt/php-8.5"));
    }

    /// 把三条线升级到新补丁；`old` 控制是否保留旧补丁。
    fn upgrade(&self, php_old: bool, openssl_old: bool, zlib_old: bool) {
        let text = fs::read_to_string(self.catalog()).unwrap();
        let text = replace_line(
            &text,
            "\"8.5\" = {",
            &format!(
                "\"8.5\" = {{ version = \"8.5.13\", url = \"https://example.com/php-8.5.13.tar.xz\"{}{} }}",
                patch_entry(php_old, "php", "8.5.12"),
                patches_close(),
            ),
        );
        let text = replace_line(
            &text,
            "\"3.5\" = {",
            &format!(
                "\"3.5\" = {{ version = \"3.5.10\", url = \"https://example.com/openssl-3.5.10.tar.gz\"{}{} }}",
                patch_entry(openssl_old, "openssl", "3.5.9"),
                patches_close(),
            ),
        );
        let text = replace_line(
            &text,
            "\"1.3\" = {",
            &format!(
                "\"1.3\" = {{ version = \"1.3.3\", url = \"https://example.com/zlib-1.3.3.tar.gz\"{}{} }}",
                patch_entry(zlib_old, "zlib", "1.3.2"),
                patches_close(),
            ),
        );
        fs::write(self.catalog(), text).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_phprt"))
            .arg("--root")
            .arg(&self.root)
            .arg("--recipes")
            .arg(self.recipes())
            .args(args)
            .output()
            .unwrap()
    }

    fn plan_json(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "plan 失败：{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn patches_close() -> &'static str {
    ""
}

fn patch_entry(enabled: bool, name: &str, version: &str) -> String {
    if !enabled {
        return String::new();
    }
    format!(
        ", patches = [ {{ version = \"{version}\", url = \"https://example.com/{name}-{version}.tar.gz\" }} ]"
    )
}

fn replace_line(text: &str, prefix: &str, replacement: &str) -> String {
    text.lines()
        .map(|line| {
            if line.trim_start().starts_with(prefix) {
                replacement
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn node<'a>(sim: &'a Value, id: &str) -> &'a Value {
    sim["order"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == id)
        .unwrap_or_else(|| panic!("计划里没有 {id}"))
}

fn version<'a>(sim: &'a Value, id: &str) -> &'a str {
    node(sim, id)["version"].as_str().unwrap()
}

fn state(components: &[(&str, &str)], deps: &[(&str, &str, &str)], installed: &str) -> Value {
    let mut component_map = serde_json::Map::new();
    for (name, mode) in components {
        component_map.insert(
            (*name).to_owned(),
            serde_json::json!({
                "line": "",
                "version": installed,
                "mode": mode,
                "installed": true,
                "deps": {},
                "features": []
            }),
        );
    }
    let mut dep_map = serde_json::Map::new();
    for (name, line, version) in deps {
        dep_map.insert(
            (*name).to_owned(),
            serde_json::json!({ "line": line, "version": version, "features": [] }),
        );
    }
    serde_json::json!({
        "version": 3,
        "active": true,
        "product": "php",
        "series": "8.5",
        "installed_version": installed,
        "bindings": {},
        "components": component_map,
        "deps": dep_map,
    })
}

fn full_state() -> Value {
    state(
        &[("openssl", "static"), ("zlib", "static")],
        &[("openssl", "3.5", "3.5.9"), ("zlib", "1.3", "1.3.2")],
        "8.5.12",
    )
}

#[test]
fn default_keeps_product_patch_but_takes_catalog_dep_patches() {
    let f = Fixture::new("default");
    f.write_state(full_state());
    f.upgrade(true, true, true);

    let sim = f.plan_json(&["plan", "php@8.5", "--json"]);
    assert_eq!(version(&sim, "product:php@8.5.12"), "8.5.12");
    assert_eq!(version(&sim, "dep:openssl"), "3.5.10");
    assert_eq!(version(&sim, "dep:zlib"), "1.3.3");
}

#[test]
fn lock_uses_the_patches_recorded_in_state() {
    let f = Fixture::new("lock");
    f.write_state(full_state());
    f.upgrade(true, true, true);

    let sim = f.plan_json(&["plan", "php@8.5", "--lock", "--json"]);
    assert_eq!(version(&sim, "product:php@8.5.12"), "8.5.12");
    assert_eq!(version(&sim, "dep:openssl"), "3.5.9");
    assert_eq!(version(&sim, "dep:zlib"), "1.3.2");
}

#[test]
fn lock_never_falls_back_when_the_old_patch_was_removed() {
    let f = Fixture::new("no-fallback");
    f.write_state(full_state());
    // php / openssl 的旧补丁还在，只删 zlib 1.3.2。
    f.upgrade(true, true, false);

    let out = f.run(&["plan", "php@8.5", "--lock", "--json"]);
    assert!(!out.status.success(), "缺旧补丁时 --lock 必须失败");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("zlib 1.3.2"), "{err}");
    assert!(err.contains("不在 catalog"), "{err}");
    // 不能偷偷用 1.3.3。
    assert!(
        !out.stdout
            .as_slice()
            .windows(b"1.3.3".len())
            .any(|w| w == b"1.3.3"),
        "失败时不该产出回落到 1.3.3 的计划"
    );
}

#[test]
fn lock_requires_an_installed_version() {
    let f = Fixture::new("not-installed");
    let mut state = full_state();
    state["installed_version"] = Value::String(String::new());
    f.write_state(state);
    f.upgrade(true, true, true);

    let out = f.run(&["plan", "php@8.5", "--lock", "--json"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--lock 没有可锁定的版本"), "{err}");
}

#[test]
fn cli_line_override_replaces_the_locked_binding() {
    let f = Fixture::new("cli-override");
    f.write_state(full_state());
    f.upgrade(true, true, true);

    let sim = f.plan_json(&[
        "plan",
        "php@8.5",
        "--lock",
        "--dep",
        "openssl=4.0",
        "--json",
    ]);
    // 显式换线后不能再残留 3.5 / 3.5.9 的 pin。
    assert_eq!(version(&sim, "dep:openssl"), "4.0.0");
    assert_eq!(node(&sim, "dep:openssl")["line"], "4.0");
}

#[test]
fn lock_ignores_state_deps_outside_the_current_closure() {
    let f = Fixture::new("stale-dep");
    // 方案只剩 openssl；state 里还留着 zlib 的历史记录。
    f.write_state(state(
        &[("openssl", "static")],
        &[("openssl", "3.5", "3.5.9"), ("zlib", "1.3", "1.3.2")],
        "8.5.12",
    ));
    // zlib 1.3.2 已经不在 catalog；如果它被当成 pin，--lock 会失败。
    f.upgrade(true, true, false);

    let sim = f.plan_json(&["plan", "php@8.5", "--lock", "--json"]);
    assert_eq!(version(&sim, "dep:openssl"), "3.5.9");
    assert!(
        !sim["order"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["id"] == "dep:zlib")
    );
}

#[test]
fn lock_does_not_pin_deps_from_removed_components() {
    let f = Fixture::new("removed-component");
    // state 里只剩 openssl；libxml2 是已删除组件留下的历史依赖。
    // 它的版本已经不在 catalog：如果被当成 pin，--lock 会报“不在 catalog”。
    let mut state = state(
        &[("openssl", "static")],
        &[("openssl", "3.5", "3.5.9")],
        "8.5.12",
    );
    state["deps"]["libxml2"] =
        serde_json::json!({ "line": "2.15", "version": "2.15.0", "features": [] });
    f.write_state(state);
    f.upgrade(true, true, true);

    let sim = f.plan_json(&["plan", "php@8.5", "--lock", "--json"]);
    assert_eq!(version(&sim, "dep:openssl"), "3.5.9");
    assert!(
        !sim["order"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["id"] == "dep:libxml2")
    );
}

#[test]
fn lock_uses_the_versions_written_by_the_last_update() {
    let f = Fixture::new("after-update");
    // update 成功后 state 记录的就是新补丁；下一次 --lock 以它为准。
    f.write_state(state(
        &[("openssl", "static"), ("zlib", "static")],
        &[("openssl", "3.5", "3.5.10"), ("zlib", "1.3", "1.3.3")],
        "8.5.13",
    ));
    f.upgrade(true, true, true);

    let sim = f.plan_json(&["plan", "php@8.5", "--lock", "--json"]);
    assert_eq!(version(&sim, "product:php@8.5.13"), "8.5.13");
    assert_eq!(version(&sim, "dep:openssl"), "3.5.10");
    assert_eq!(version(&sim, "dep:zlib"), "1.3.3");
}

#[test]
fn product_patch_without_state_pin_reports_a_catalog_error() {
    let f = Fixture::new("product-missing");
    f.write_state(full_state());
    // 只删 php 8.5.12；默认模式也不许回落到 8.5.13。
    f.upgrade(false, true, true);

    let out = f.run(&["plan", "php@8.5", "--json"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("state 里的 php 8.5.12"), "{err}");
}

#[test]
fn scheme_lock_exports_product_and_exact_patches() {
    let f = Fixture::new("scheme");
    f.write_state(full_state());
    f.upgrade(true, true, true);

    let out = f.run(&["scheme", "php@8.5", "--lock"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(line.contains("phprt install php@8.5.12"), "{line}");
    assert!(line.contains("--dep openssl=3.5.9"), "{line}");
    assert!(line.contains("--dep zlib=1.3.2"), "{line}");
    assert!(!line.contains("php@8.5.13"), "{line}");
    // bundled 组件的 version 是产品补丁，不能写成它自己的版本。
    assert!(!line.contains("version:8.5.12"), "{line}");
}

#[test]
fn scheme_lock_refuses_a_patch_missing_from_catalog() {
    let f = Fixture::new("scheme-missing");
    f.write_state(full_state());
    f.upgrade(true, true, false);

    let out = f.run(&["scheme", "php@8.5", "--lock"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("zlib 1.3.2"), "{err}");
}

#[test]
fn no_state_means_no_lock_source() {
    let f = Fixture::new("no-state");
    f.remove_state();
    f.upgrade(true, true, true);

    let out = f.run(&["plan", "php@8.5", "--lock", "--json"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--lock 没有可锁定的版本"), "{err}");
}
