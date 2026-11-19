//! `--targets` 的端到端测试：只跑 `plan`，不下载、不编译。

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
            std::env::temp_dir().join(format!("phprt-targets-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("recipes");
        copy_dir(&source, &root.join("recipes"));
        Self { root }
    }

    fn recipes(&self) -> PathBuf {
        self.root.join("recipes")
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

    fn plan(&self, targets: &str) -> Value {
        let out = self.run(&["plan", "php@8.5", "--json", "--targets", targets]);
        assert!(
            out.status.success(),
            "plan 失败：{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    fn plan_default(&self) -> Value {
        let out = self.run(&["plan", "php@8.5", "--json"]);
        assert!(
            out.status.success(),
            "plan 失败：{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

fn product_configure(plan: &Value) -> Vec<String> {
    plan["order"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["kind"] == "product")
        .unwrap()["configure"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

fn has(flags: &[String], flag: &str) -> bool {
    flags.iter().any(|item| item == flag)
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let dest = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &dest);
        } else {
            fs::copy(&path, &dest).unwrap();
        }
    }
}

#[test]
fn default_targets_come_from_bundle() {
    let fixture = Fixture::new("default");
    let plan = fixture.plan_default();
    assert_eq!(plan["targets"], serde_json::json!({"cli": {}}));
    let flags = product_configure(&plan);
    assert!(has(&flags, "--enable-cli"), "{flags:?}");
    assert!(has(&flags, "--disable-cgi"), "{flags:?}");
    assert!(!has(&flags, "--enable-fpm"), "{flags:?}");
    assert!(!has(&flags, "--disable-fpm"), "{flags:?}");
    let _ = fs::remove_dir_all(&fixture.root);
}

#[test]
fn selected_and_unselected_targets_apply_their_flags() {
    let fixture = Fixture::new("flags");
    let plan = fixture.plan("cli,fpm=user:www-data,group:www-data");
    let flags = product_configure(&plan);
    assert!(has(&flags, "--enable-cli"), "{flags:?}");
    assert!(has(&flags, "--enable-fpm"), "{flags:?}");
    assert!(has(&flags, "--disable-cgi"), "{flags:?}");
    assert!(has(&flags, "--with-fpm-user=www-data"), "{flags:?}");
    assert!(has(&flags, "--with-fpm-group=www-data"), "{flags:?}");
    assert!(!has(&flags, "--disable-fpm"), "{flags:?}");
    let _ = fs::remove_dir_all(&fixture.root);
}

#[test]
fn unselected_cli_gets_disable_flag() {
    let fixture = Fixture::new("fpm-only");
    let plan = fixture.plan("fpm");
    let flags = product_configure(&plan);
    assert!(has(&flags, "--disable-cli"), "{flags:?}");
    assert!(has(&flags, "--enable-fpm"), "{flags:?}");
    assert!(!has(&flags, "--disable-fpm"), "{flags:?}");
    let _ = fs::remove_dir_all(&fixture.root);
}

#[test]
fn target_order_does_not_change_identity() {
    let fixture = Fixture::new("order");
    let a = fixture.plan("cli,fpm=user:www-data,group:www-data");
    let b = fixture.plan("fpm=group:www-data,user:www-data,cli");
    assert_eq!(a["targets"], b["targets"]);
    let _ = fs::remove_dir_all(&fixture.root);
}

#[test]
fn unknown_target_and_config_are_errors() {
    let fixture = Fixture::new("errors");
    for (spec, expected) in [
        ("nope", "不受"),
        ("cli=user:x", "没有配置项"),
        ("fpm=user:[a,b]", "不能传列表"),
    ] {
        let out = fixture.run(&["plan", "php@8.5", "--targets", spec]);
        assert!(!out.status.success(), "{spec} 应该失败");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(expected), "{spec}: {err}");
    }
    let _ = fs::remove_dir_all(&fixture.root);
}

#[test]
fn products_without_targets_reject_the_flag() {
    let fixture = Fixture::new("nginx");
    let out = fixture.run(&["plan", "nginx@1.29", "--targets", "cli"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("没有声明 targets"), "{err}");
    let _ = fs::remove_dir_all(&fixture.root);
}

#[test]
fn add_does_not_accept_targets() {
    let fixture = Fixture::new("add");
    let out = fixture.run(&["add", "php@8.5", "swoole", "--targets", "cli"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unexpected argument"), "{err}");
    let _ = fs::remove_dir_all(&fixture.root);
}
