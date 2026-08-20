//! Shared helpers for the integration-test binaries.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

/// Path to the compiled `gwen` binary.
pub fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_gwen")
}

/// A fresh scratch directory, unique per call.
pub fn tmp() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "gwen-it-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `gwen` with `args`, asserting success, and return stdout.
pub fn run_ok(args: &[&str]) -> String {
    let out = Command::new(bin()).args(args).output().expect("run binary");
    assert!(
        out.status.success(),
        "command failed: {args:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// Create a new project from `template` inside a scratch dir; returns the
/// project directory.
pub fn new_project(template: &Path, name: &str) -> PathBuf {
    let dir = tmp();
    let project = dir.join(name);
    run_ok(&[
        "new",
        project.to_str().unwrap(),
        "--pptx",
        template.to_str().unwrap(),
    ]);
    project
}

/// Build a project directory and return the output `.pptx`.
pub fn build_project(project: &Path) -> PathBuf {
    run_ok(&["build", project.to_str().unwrap()]);
    let name = project_name(project);
    project.join("target").join(format!("{name}.pptx"))
}

/// The `[presentation] name` from the project's config.toml.
pub fn project_name(project: &Path) -> String {
    let raw = std::fs::read_to_string(project.join("config.toml")).unwrap();
    let v: toml::Value = toml::from_str(&raw).unwrap();
    v["presentation"]["name"].as_str().unwrap().to_string()
}

/// The text of the project's src/PRESENTATION.md mirror.
pub fn project_md(project: &Path) -> String {
    std::fs::read_to_string(project.join("src").join("PRESENTATION.md")).unwrap()
}

/// The raw text of a zip entry inside a deck.
pub fn read_zip_entry(path: &Path, name: &str) -> String {
    String::from_utf8(read_zip_bytes(path, name)).unwrap()
}

/// The raw bytes of a zip entry inside a deck.
pub fn read_zip_bytes(path: &Path, name: &str) -> Vec<u8> {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut entry = zip.by_name(name).unwrap();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
    buf
}
