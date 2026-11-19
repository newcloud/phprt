//! `--lock` 的端到端测试：只跑 `plan` / `scheme`，不做下载和编译。
//!
//! 夹具是临时项目根 + 复制一份 recipes，改 catalog 模拟“catalog 升级但保留旧补丁”。

use std::collections::BTreeMap;
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

    fn read_state(&self) -> Value {
        let text = fs::read_to_string(self.root.join(".phprt/php-8.5/state.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    /// 模拟 catalog 删掉 libmemcached 的旧线 1.1：线改名为 1.2，bundle 同步。
    fn retire_libmemcached_line(&self) {
        let text = fs::read_to_string(self.catalog()).unwrap();
        let text = replace_line(
            &text,
            "\"1.1\" = {",
            "\"1.2\" = { version = \"1.1.4\", url = \"https://github.com/awesomized/libmemcached/archive/refs/tags/1.1.4.tar.gz\", sha256 = \"c477e1f6510e1dc698e84f3717ce690a8f65b94c616ecaa62306cce0f5e3116a\" }",
        );
        fs::write(self.catalog(), text).unwrap();

        let bundle = self.recipes().join("bundles/php/8.5.toml");
        let text = fs::read_to_string(&bundle).unwrap();
        let text = replace_line(&text, "libmemcached = ", "libmemcached = \"1.2\"");
        fs::write(bundle, text).unwrap();
    }

    fn remove_state(&self) {
        let _ = fs::remove_dir_all(self.root.join(".phprt/php-8.5"));
    }

    /// 加一条按 line 改变 needs 的 swoole 线：6.3 的 `LINES` 里没有 openssl，
    /// 因此它的 `needs` 只有 libcurl。用来验证两遍解析之间的闭包校验。
    fn add_openssl_free_swoole_line(&self) {
        let catalog = self.catalog();
        let text = fs::read_to_string(&catalog).unwrap();
        let old = "\"6.2\" = { version = \"6.2.5\", url = \"https://pecl.php.net/get/swoole-6.2.5.tgz\", sha256 = \"edafabbfe5eb125a2be4c83e437667a83b7d7aabc801ba410ff92724ba83e88c\" }";
        assert!(text.contains(old), "catalog 的 swoole 6.2 行变了");
        let text = text.replace(
            old,
            &format!(
                "{old}\n\"6.3\" = {{ version = \"6.3.0\", url = \"https://example.com/swoole-6.3.0.tgz\" }}"
            ),
        );
        fs::write(&catalog, text).unwrap();

        let recipe = self.recipes().join("components/php/swoole.star");
        let text = fs::read_to_string(&recipe).unwrap();
        let old = "    \"6.2\": {\"product\": [\"8.4\", \"8.5\"], \"openssl\": [\"1.1.1\", \"3.0\", \"3.5\", \"4.0\"]},";
        assert!(text.contains(old), "swoole.star 的 6.2 行变了");
        let text = text.replace(
            old,
            &format!("{old}\n    \"6.3\": {{\"product\": [\"8.4\", \"8.5\", \"8.6\"]}},"),
        );
        fs::write(&recipe, text).unwrap();
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

/// 装了 memcached，并在 bindings 里显式钉了它的依赖 libmemcached。
fn memcached_state() -> Value {
    let mut st = state(
        &[],
        &[("libmemcached", "1.1", "1.1.4"), ("zlib", "1.3", "1.3.2")],
        "8.5.12",
    );
    st["components"]["memcached"] = serde_json::json!({
        "line": "3.4",
        "version": "3.4.0",
        "mode": "shared",
        "installed": true,
        "deps": {},
        "features": []
    });
    st["bindings"] = serde_json::json!({ "libmemcached": "1.1" });
    st
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

#[test]
fn remove_prunes_bindings_outside_the_closure() {
    let f = Fixture::new("prune-bindings");
    f.write_state(memcached_state());

    let out = f.run(&["remove", "php@8.5", "memcached"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let saved = f.read_state();
    assert!(saved["bindings"].as_object().unwrap().is_empty(), "{saved}");
    assert!(
        !saved["deps"]
            .as_object()
            .unwrap()
            .contains_key("libmemcached"),
        "{saved}"
    );
}

#[test]
fn stale_binding_does_not_block_later_commands() {
    let f = Fixture::new("stale-later");
    f.write_state(memcached_state());

    let out = f.run(&["remove", "php@8.5", "memcached"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // remove 之后 catalog 再删旧线：后续命令不该再碰到这条 binding。
    f.retire_libmemcached_line();

    let out = f.run(&["plan", "php@8.5", "--json"]);
    assert!(
        out.status.success(),
        "plan: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = f.run(&["add", "php@8.5", "xdebug"]);
    assert!(
        out.status.success(),
        "add: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = f.run(&["remove", "php@8.5", "xdebug"]);
    assert!(
        out.status.success(),
        "remove: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // update 会进入下载阶段（离线空缓存会失败），这里只验证 resolver
    // 没有被 stale binding 挡住。
    let out = f.run(&["update", "php@8.5", "--downloads", "--offline"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("libmemcached"), "{err}");
}

#[test]
fn scheme_lock_skips_bindings_outside_the_closure() {
    let f = Fixture::new("scheme-stale-binding");
    let mut st = full_state();
    st["bindings"] = serde_json::json!({ "libmemcached": "1.1" });
    f.write_state(st);

    let out = f.run(&["scheme", "php@8.5", "--lock"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = String::from_utf8_lossy(&out.stdout);
    assert!(!line.contains("libmemcached"), "{line}");
    assert!(line.contains("--dep openssl=3.5.9"), "{line}");
    assert!(line.contains("--dep zlib=1.3.2"), "{line}");
}

#[test]
fn missing_stale_line_does_not_block_remove() {
    let f = Fixture::new("stale-blocks-remove");
    f.write_state(memcached_state());
    // catalog 已经删掉旧线，binding 还留在 state 里：remove 自己不能被它挡住。
    f.retire_libmemcached_line();

    let out = f.run(&["remove", "php@8.5", "memcached"]);
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let saved = f.read_state();
    assert!(
        !saved["bindings"]
            .as_object()
            .unwrap()
            .contains_key("libmemcached"),
        "{saved}"
    );
    assert!(
        !saved["deps"]
            .as_object()
            .unwrap()
            .contains_key("libmemcached"),
        "{saved}"
    );
}

/// `plan` / `install --downloads` 共用的文本输出解析：只取和复现有关的字段。
#[derive(Debug, PartialEq, Eq)]
struct PlanNode {
    version: String,
    line: String,
    mode: String,
    needs: Vec<String>,
    features: Vec<String>,
    deps: Vec<String>,
}

/// 从 `render::to_text` 的输出里取出节点集合与每个节点的输入。
///
/// `build order` 段给组件的 mode；后面的 `[i/N]` 段给 version / line /
/// needs / features / dep。所有列表都排序，避免只比较顺序。
fn plan_nodes(text: &str) -> BTreeMap<String, PlanNode> {
    let mut modes: BTreeMap<String, String> = BTreeMap::new();
    let mut nodes: BTreeMap<String, PlanNode> = BTreeMap::new();
    let mut in_order = false;
    let mut current: Option<String> = None;

    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed == "build order" {
            in_order = true;
            continue;
        }
        if in_order {
            if trimmed.is_empty() {
                in_order = false;
                continue;
            }
            let Some((index, rest)) = trimmed.split_once(". ") else {
                continue;
            };
            if index.parse::<usize>().is_err() {
                continue;
            }
            let id = rest.split_whitespace().next().unwrap().to_owned();
            let tail = rest[id.len()..].trim();
            let mode = if tail.starts_with('(') {
                let inner = tail.trim_start_matches('(').trim_end_matches(')');
                inner
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_owned()
            } else {
                String::new()
            };
            modes.insert(id, mode);
            continue;
        }

        if trimmed.starts_with('[') && trimmed.contains("] ") {
            let rest = &trimmed[1..];
            let (index, tail) = rest.split_once("] ").unwrap();
            if !index.contains('/') {
                continue;
            }
            let id = tail.split_whitespace().next().unwrap().to_owned();
            let version = tail[id.len()..].trim().to_owned();
            let mode = modes.get(&id).cloned().unwrap_or_default();
            current = Some(id.clone());
            nodes.insert(
                id,
                PlanNode {
                    version,
                    line: String::new(),
                    mode,
                    needs: Vec::new(),
                    features: Vec::new(),
                    deps: Vec::new(),
                },
            );
            continue;
        }

        let Some(id) = current.clone() else { continue };
        let Some((key, value)) = trimmed.split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
        let node = nodes.get_mut(&id).unwrap();
        match key {
            "line" => node.line = value.to_owned(),
            "needs" => node.needs = sorted_list(value),
            "features" => node.features = sorted_list(value),
            "dep" => node.deps.push(value.to_owned()),
            _ => {}
        }
    }

    for node in nodes.values_mut() {
        node.deps.sort();
    }
    nodes
}

fn sorted_list(value: &str) -> Vec<String> {
    let mut items: Vec<String> = value.split(", ").map(str::to_owned).collect();
    items.sort();
    items
}

/// 把导出的 `phprt install ...` 命令行拆成参数。
fn command_args(line: &str) -> Vec<String> {
    line.split_whitespace()
        .skip(1)
        .map(|item| item.trim_matches('\'').to_owned())
        .collect()
}

/// `scheme --lock` 导出的命令要能在干净 root 里复现同一份解析结果。
#[test]
fn scheme_lock_round_trips_the_resolved_graph() {
    let source = Fixture::new("roundtrip-src");
    // 完整方案 + 依赖；故意不给 zlib 记 version，并留一条闭包外的历史依赖。
    let mut st = state(
        &[
            ("openssl", "static"),
            ("curl", "static"),
            ("libxml", "static"),
            ("zlib", "static"),
        ],
        &[
            ("openssl", "3.5", "3.5.9"),
            ("libcurl", "8.23", "8.23.0"),
            ("libxml2", "2.15", "2.15.4"),
            ("libmemcached", "1.1", "1.1.4"),
        ],
        "8.5.12",
    );
    st["components"]["swoole"] = serde_json::json!({
        "line": "6.2",
        "version": "6.2.5",
        "mode": "shared",
        "installed": true,
        "deps": {},
        "features": []
    });
    st["components"]["curl"]["deps"] = serde_json::json!({ "libcurl": ["ssl"] });
    st["components"]["openssl"]["features"] = serde_json::json!(["argon2"]);
    st["bindings"] = serde_json::json!({ "libmemcached": "1.1" });
    source.write_state(st);

    let out = source.run(&["plan", "php@8.5"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let before = plan_nodes(&String::from_utf8_lossy(&out.stdout));
    assert!(!before.contains_key("dep:libmemcached"), "{before:#?}");

    let out = source.run(&["scheme", "php@8.5", "--lock"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cmd = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    // state 没有 zlib 的 version 记录：用本次解析选中的 artifact 补齐。
    assert!(cmd.contains("--dep zlib=1.3.2"), "{cmd}");
    assert!(!cmd.contains("libmemcached"), "{cmd}");

    let target = Fixture::new("roundtrip-dst");
    let mut args = command_args(&cmd);
    args.push("--downloads".into());
    args.push("--offline".into());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = target.run(&refs);
    // 离线空缓存会在下载阶段失败；解析结果在失败之前就打印好了。
    let text = String::from_utf8_lossy(&out.stdout);
    let after = plan_nodes(&text);
    assert!(!after.is_empty(), "新 root 没有打印计划：{text}");

    assert!(before.contains_key("product:php@8.5.12"), "{before:#?}");
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>()
    );
    for (id, node) in &before {
        assert_eq!(node, &after[id], "{id}");
    }
}

/// 第二遍换线改变 needs 时，闭包校验必须拦住 `--lock`，普通命令只警告。
#[test]
fn second_pass_line_change_is_checked() {
    let f = Fixture::new("closure-change");
    f.add_openssl_free_swoole_line();
    let mut st = state(
        &[],
        &[("openssl", "3.5", "3.5.9"), ("libcurl", "8.23", "8.23.0")],
        "8.5.12",
    );
    st["components"]["swoole"] = serde_json::json!({
        "line": "6.3",
        "version": "6.3.0",
        "mode": "shared",
        "installed": true,
        "deps": {},
        "features": []
    });
    f.write_state(st);

    // 第一遍按 bundle 的 6.2 算闭包（含 openssl），第二遍换成 state 的 6.3（不含）。
    let out = f.run(&["plan", "php@8.5"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("依赖闭包变了"), "{err}");
    assert!(err.contains("dep:openssl"), "{err}");

    let out = f.run(&["plan", "php@8.5", "--lock"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("依赖闭包变了"), "{err}");
    assert!(err.contains("dep:openssl"), "{err}");
    assert!(err.contains("--lock"), "{err}");
}
