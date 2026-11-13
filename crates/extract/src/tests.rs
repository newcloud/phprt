use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use flate2::Compression;
use flate2::write::GzEncoder;

use phprt_source::Source;
use phprt_ui::Mode;

use super::run;

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(name: &str) -> PathBuf {
    let n = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("phprt-extract-{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("建测试目录");
    dir
}

fn tar_bytes(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, data, mode) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mode(*mode);
        header.set_mtime(0);
        builder
            .append_data(&mut header, path, *data)
            .expect("写 tar 条目");
    }
    builder.into_inner().expect("结束 tar")
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("gzip");
    encoder.finish().expect("gzip finish")
}

fn xz(data: &[u8]) -> Vec<u8> {
    let mut writer = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(6))
        .expect("xz writer");
    writer.write_all(data).expect("xz");
    writer.finish().expect("xz finish")
}

fn bzip2(data: &[u8]) -> Vec<u8> {
    let mut writer = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    writer.write_all(data).expect("bzip2");
    writer.finish().expect("bzip2 finish")
}

fn zip_bytes(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, data, mode) in entries {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(*mode);
        writer.start_file(*path, options).expect("写 zip 条目");
        writer.write_all(data).expect("写 zip 数据");
    }
    writer.finish().expect("结束 zip").into_inner()
}

fn source(dir: &Path, name: &str, stem: &str) -> Source {
    Source {
        label: "deps".to_owned(),
        name: "pkg".to_owned(),
        version: "1.0".to_owned(),
        url: "https://example.com/pkg.tar.gz".to_owned(),
        sha256: String::new(),
        archive: dir.join(name),
        unpack: dir.join("build").join("pkg").join(stem),
        unpack_display: format!("build/pkg/{stem}"),
    }
}

fn check_tree(unpack: &Path) {
    assert_eq!(
        fs::read_to_string(unpack.join("configure")).unwrap(),
        "#!/bin/sh\n"
    );
    assert_eq!(
        fs::read_to_string(unpack.join("src/a.c")).unwrap(),
        "int main(void) { return 0; }\n"
    );
}

#[cfg(unix)]
fn assert_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path).unwrap().permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "可执行位丢了: {mode:o}");
}

#[test]
fn extracts_tar_gz_and_flattens_top_dir() {
    let dir = temp_dir("tgz");
    let tar = tar_bytes(&[
        ("pkg-1.0/configure", b"#!/bin/sh\n", 0o755),
        ("pkg-1.0/src/a.c", b"int main(void) { return 0; }\n", 0o644),
    ]);
    let archive = dir.join("pkg-1.0.tar.gz");
    fs::write(&archive, gzip(&tar)).unwrap();

    run(&[source(&dir, "pkg-1.0.tar.gz", "pkg-1.0")], Mode::Plain).unwrap();

    let unpack = dir.join("build/pkg/pkg-1.0");
    check_tree(&unpack);
    assert!(!unpack.join("pkg-1.0").exists(), "顶层目录应该被提平");
    #[cfg(unix)]
    assert_executable(&unpack.join("configure"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn extracts_tar_xz() {
    let dir = temp_dir("txz");
    let tar = tar_bytes(&[
        ("pkg-1.0/configure", b"#!/bin/sh\n", 0o755),
        ("pkg-1.0/src/a.c", b"int main(void) { return 0; }\n", 0o644),
    ]);
    fs::write(dir.join("pkg-1.0.tar.xz"), xz(&tar)).unwrap();

    run(&[source(&dir, "pkg-1.0.tar.xz", "pkg-1.0")], Mode::Plain).unwrap();
    check_tree(&dir.join("build/pkg/pkg-1.0"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn extracts_tar_bz2() {
    let dir = temp_dir("tbz2");
    let tar = tar_bytes(&[
        ("pkg-1.0/configure", b"#!/bin/sh\n", 0o755),
        ("pkg-1.0/src/a.c", b"int main(void) { return 0; }\n", 0o644),
    ]);
    fs::write(dir.join("pkg-1.0.tar.bz2"), bzip2(&tar)).unwrap();

    run(&[source(&dir, "pkg-1.0.tar.bz2", "pkg-1.0")], Mode::Plain).unwrap();
    check_tree(&dir.join("build/pkg/pkg-1.0"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn extracts_zip() {
    let dir = temp_dir("zip");
    let data = zip_bytes(&[
        ("pkg-1.0/configure", b"#!/bin/sh\n", 0o755),
        ("pkg-1.0/src/a.c", b"int main(void) { return 0; }\n", 0o644),
    ]);
    fs::write(dir.join("pkg-1.0.zip"), data).unwrap();

    run(&[source(&dir, "pkg-1.0.zip", "pkg-1.0")], Mode::Plain).unwrap();
    check_tree(&dir.join("build/pkg/pkg-1.0"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn flat_archive_stays_flat() {
    let dir = temp_dir("flat");
    let tar = tar_bytes(&[("configure", b"#!/bin/sh\n", 0o755)]);
    fs::write(dir.join("pkg-1.0.tar.gz"), gzip(&tar)).unwrap();

    run(&[source(&dir, "pkg-1.0.tar.gz", "pkg-1.0")], Mode::Plain).unwrap();
    assert_eq!(
        fs::read_to_string(dir.join("build/pkg/pkg-1.0/configure")).unwrap(),
        "#!/bin/sh\n"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn skips_when_unpack_dir_is_not_empty() {
    let dir = temp_dir("skip");
    let unpack = dir.join("build/pkg/pkg-1.0");
    fs::create_dir_all(&unpack).unwrap();
    fs::write(unpack.join("already"), b"x").unwrap();

    run(&[source(&dir, "pkg-1.0.tar.gz", "pkg-1.0")], Mode::Plain).unwrap();
    assert!(unpack.join("already").exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn empty_unpack_dir_is_recreated() {
    let dir = temp_dir("empty");
    let tar = tar_bytes(&[("pkg-1.0/configure", b"#!/bin/sh\n", 0o755)]);
    fs::write(dir.join("pkg-1.0.tar.gz"), gzip(&tar)).unwrap();
    let unpack = dir.join("build/pkg/pkg-1.0");
    fs::create_dir_all(&unpack).unwrap();

    run(&[source(&dir, "pkg-1.0.tar.gz", "pkg-1.0")], Mode::Plain).unwrap();
    assert!(unpack.join("configure").is_file());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unknown_format_is_reported() {
    let dir = temp_dir("unknown");
    fs::write(dir.join("pkg-1.0.tar.zst"), b"x").unwrap();
    let err = run(&[source(&dir, "pkg-1.0.tar.zst", "pkg-1.0")], Mode::Plain).unwrap_err();
    assert!(format!("{err:#}").contains("不认识的压缩格式"), "{err:#}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_archive_is_reported() {
    let dir = temp_dir("missing");
    let err = run(&[source(&dir, "pkg-1.0.tar.gz", "pkg-1.0")], Mode::Plain).unwrap_err();
    assert!(format!("{err:#}").contains("源码包不在缓存"), "{err:#}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_archive_leaves_no_destination() {
    let dir = temp_dir("corrupt");
    fs::write(dir.join("pkg-1.0.tar.gz"), b"not gzip").unwrap();
    let result = run(&[source(&dir, "pkg-1.0.tar.gz", "pkg-1.0")], Mode::Plain);

    assert!(result.is_err());
    assert!(!dir.join("build/pkg/pkg-1.0").exists(), "不应留下半份源码");
    let leftovers: Vec<_> = fs::read_dir(dir.join("build/pkg"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(leftovers.is_empty(), "临时目录没清掉: {leftovers:?}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tar_traversal_is_rejected() {
    let dir = temp_dir("traversal");
    fs::write(dir.join("evil.tar.gz"), gzip(&evil_tar())).unwrap();
    let result = run(&[source(&dir, "evil.tar.gz", "evil")], Mode::Plain);

    assert!(result.is_err());
    assert!(!dir.join("evil.txt").exists(), "越界文件被写出来了");
    assert!(!dir.join("build/pkg/evil").exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn extracts_pecl_package_xml_layout() {
    let dir = temp_dir("pecl");
    let tar = tar_bytes(&[
        ("package.xml", b"<package/>\n", 0o644),
        ("pkg-1.0/configure", b"#!/bin/sh\n", 0o755),
        ("pkg-1.0/src/a.c", b"int main(void) { return 0; }\n", 0o644),
    ]);
    let archive = dir.join("pkg-1.0.tgz");
    fs::write(&archive, gzip(&tar)).unwrap();

    run(&[source(&dir, "pkg-1.0.tgz", "pkg-1.0")], Mode::Plain).unwrap();

    let unpack = dir.join("build/pkg/pkg-1.0");
    check_tree(&unpack);
    assert!(!unpack.join("package.xml").exists());
    assert!(!unpack.join("pkg-1.0").exists());

    let _ = fs::remove_dir_all(&dir);
}

/// 手工拼一个顶层是 `../evil.txt` 的 tar，写入端会拒绝这种路径，
/// 只能直接构造 header 来测解压端。
fn evil_tar() -> Vec<u8> {
    let mut header = [0u8; 512];
    let name = b"../evil.txt";
    header[..name.len()].copy_from_slice(name);
    header[100..107].copy_from_slice(b"0000644");
    header[108..115].copy_from_slice(b"0000000");
    header[116..123].copy_from_slice(b"0000000");
    header[124..135].copy_from_slice(b"00000000000");
    header[136..147].copy_from_slice(b"00000000000");
    header[148..156].copy_from_slice(b"        ");
    header[156] = b'0';
    let sum: u32 = header.iter().map(|byte| *byte as u32).sum();
    let checksum = format!("{sum:06o}\0 ");
    header[148..156].copy_from_slice(checksum.as_bytes());
    let mut out = header.to_vec();
    out.extend_from_slice(&[0u8; 1024]);
    out
}
