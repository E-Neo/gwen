//! End-to-end smoke tests: scaffold a project, build it, and verify the output
//! is a real `.pptx` package. Content mapping is covered by unit tests on
//! `render::spec`; here we only assert the packaged artifact is well-formed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

fn tmp(name: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "gwen-v3-{}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// A minimal project: two slides, no media.
fn sample_project(name: &str) -> PathBuf {
    let dir = tmp(name);
    write(
        &dir.join("config.toml"),
        r##"[presentation]
name = "deck"
slide_width = 12192000
slide_height = 6858000
default_layout = "title"

[theme]
major_font = "Calibri"
minor_font = "Calibri"

[[layouts.title.elements]]
kind = "slot"
slot = "title"
type = "text"
left = 914400
top = 2743200
width = 10363200
height = 1371600
align = "center"
anchor = "middle"
text_size = 40
color = "#1D1D1A"

[[layouts.content.elements]]
kind = "slot"
slot = "title"
type = "text"
left = 914400
top = 685800
width = 10363200
height = 914400
align = "left"
anchor = "middle"
text_size = 32
color = "#1D1D1A"

[[layouts.content.elements]]
kind = "slot"
slot = "body"
type = "text"
left = 914400
top = 1828800
width = 10363200
height = 4114800
align = "left"
anchor = "top"
text_size = 20
color = "#262626"
"##,
    );
    write(
        &dir.join("src").join("SUMMARY.md"),
        "# Summary\n\n- [Title](slides/title.md)\n- [Content](slides/content.md)\n",
    );
    write(
        &dir.join("src").join("slides").join("title.md"),
        "---\nlayout: title\n---\n\n# My Deck\n\n## A gwen presentation\n",
    );
    write(
        &dir.join("src").join("slides").join("content.md"),
        "---\nlayout: content\nbackground: \"#112233\"\n---\n\n# First slide\n\n- point one\n- point two\n\n## Notes\n\nremember this\n",
    );
    std::fs::create_dir_all(dir.join("src").join("media")).unwrap();
    dir
}

#[test]
fn build_produces_a_pptx_package() {
    let dir = sample_project("basic");
    let out = gwen::build(&dir).unwrap();
    assert!(out.is_file(), "output file exists");

    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.len() > 1_000, "deck is unexpectedly tiny");
    assert_eq!(
        &bytes[0..4],
        b"PK\x03\x04",
        "starts with a zip local header"
    );
    // A zip ends with its EOCD record (22 bytes, signature first).
    assert_eq!(
        &bytes[bytes.len() - 22..bytes.len() - 18],
        b"PK\x05\x06",
        "ends with EOCD"
    );
}

#[test]
fn build_is_deterministic() {
    let dir = sample_project("deterministic");
    let first = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    let second = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    assert_eq!(first, second, "building twice yields identical bytes");
}
