//! Shared helpers for the integration-test binaries.
#![allow(dead_code)]

use std::io::Read;
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

/// The `markdown` mirror of `input`.
pub fn markdown(input: &Path) -> String {
    run_ok(&["markdown", "--input", input.to_str().unwrap()])
}

/// Apply a markdown mirror to `input`, producing a new deck in a scratch dir.
pub fn build(input: &Path, md: &str) -> PathBuf {
    let dir = tmp();
    let md_file = dir.join("deck.md");
    std::fs::write(&md_file, md).unwrap();
    let output = dir.join("out.pptx");
    run_ok(&[
        "build",
        "--input",
        input.to_str().unwrap(),
        "--markdown",
        md_file.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    output
}

/// The raw text of a zip entry inside a deck.
pub fn read_zip_entry(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut entry = zip.by_name(name).unwrap();
    let mut buf = String::new();
    Read::read_to_string(&mut entry, &mut buf).unwrap();
    buf
}

/// The block for slide `n` (0-based): everything after its `## ` heading up to
/// the next `## `/`# ` heading (or end of mirror).
pub fn slide_block(md: &str, n: usize) -> String {
    let idxs: Vec<usize> = md.match_indices("\n## ").map(|(i, _)| i + 1).collect();
    assert!(idxs.len() > n, "slide {n} not found");
    let start = idxs[n];
    let line_end = md[start..]
        .find('\n')
        .map(|i| start + i + 1)
        .unwrap_or(md.len());
    let end = match idxs.get(n + 1) {
        Some(&next) => {
            let master = md[next..]
                .find("\n# ")
                .map(|j| next + j)
                .unwrap_or(md.len());
            next.min(master)
        }
        None => md.len(),
    };
    md[line_end..end].trim().to_string()
}
