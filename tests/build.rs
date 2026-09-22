//! End-to-end smoke tests: scaffold a TOML project, build it, and verify the
//! output is a real `.pptx` package. Content mapping is covered by unit tests
//! on `render::spec`; here we only assert the packaged artifact is well-formed
//! and that `gwen new` produces a buildable project.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use base64::Engine;

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

/// A 1x1 PNG, decoded at runtime so the repo keeps no binary fixtures.
fn pixel_png() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==")
        .unwrap()
}

/// A minimal TOML project: one master, two slides, one image.
fn sample_project(name: &str) -> PathBuf {
    let dir = tmp(name);
    write(
        &dir.join("main.toml"),
        r##"[presentation]
title = "deck"
width = "13.333in"
height = "7.5in"

[theme]
major_font = "Arial"
minor_font = "Arial"

[defaults.text]
font_face = "Arial"
color = "262626"

[styles.muted]
color = "808080"
italic = true

[[sections]]
title = "Intro"
slides = ["title.toml", "content.toml"]
"##,
    );
    write(
        &dir.join("masters").join("brand.toml"),
        r##"background = { color = "FFFFFF" }
slide_number = { x = "12.2in", y = "7.1in", w = "1in", h = "0.3in", font_size = 12 }

[[shapes]]
type = "rect"
x = 0
y = 0
w = "13.333in"
h = "1.16in"
fill = { color = "C7000A" }

[[shapes]]
type = "placeholder"
ph_type = "title"
name = "Title"
x = "0.8in"
y = "0.25in"
w = "11.7in"
h = "0.66in"
font_size = 26
color = "FFFFFF"
bold = true

[[shapes]]
type = "text"
x = "0.8in"
y = "1.2in"
w = "3in"
h = "0.3in"
text = "Acme"
bold = true
"##,
    );
    write(
        &dir.join("slides").join("title.toml"),
        r##"master = "brand"

[[shapes]]
type = "text"
placeholder = "Title"
text = "*Welcome* to **gwen**"
"##,
    );
    write(
        &dir.join("slides").join("content.toml"),
        r##"master = "brand"
background = "112233"

[[shapes]]
type = "text"
x = "0.8in"
y = "1.5in"
w = "7in"
h = "4in"
text = "line one\n\nline two"
style = "muted"
font_size = 20

[[shapes]]
type = "rect"
x = "8.5in"
y = "2in"
w = "3in"
h = "1in"
fill = { color = "C7000A" }
line = { color = "000000", width = 1 }

[[shapes]]
type = "image"
x = "0.8in"
y = "5.5in"
w = "1in"
h = "1in"
src = "media/pixel.png"

notes = "remember && escape <!--"
"##,
    );
    std::fs::create_dir_all(dir.join("media")).unwrap();
    std::fs::write(dir.join("media").join("pixel.png"), pixel_png()).unwrap();
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

#[test]
fn unknown_master_is_reported() {
    let dir = sample_project("badmaster");
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"nope\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("unknown master `nope`"),
        "expected a clear error, got: {err:?}"
    );
}

fn sections_free_project(name: &str) -> PathBuf {
    let dir = sample_project(name);
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let cut = main.replace(
        "[[sections]]\ntitle = \"Intro\"\nslides = [\"title.toml\", \"content.toml\"]\n",
        "",
    );
    std::fs::write(dir.join("main.toml"), cut).unwrap();
    dir
}

#[test]
fn missing_sections_is_reported() {
    let dir = sections_free_project("nosections");
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("[[sections]]"),
        "expected an error about [[sections]], got: {err:?}"
    );
}

#[test]
fn bad_ph_type_is_reported() {
    let dir = sample_project("badphtype");
    write(
        &dir.join("masters").join("brand.toml"),
        "[[shapes]]\ntype = \"placeholder\"\nname = \"Title\"\nph_type = \"weird\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("ph_type") && msg.contains("weird"),
        "expected a ph_type error, got: {msg}"
    );
}

#[test]
fn missing_placeholder_name_is_reported() {
    let dir = sample_project("noname");
    write(
        &dir.join("masters").join("brand.toml"),
        "[[shapes]]\ntype = \"placeholder\"\nph_type = \"title\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("needs a `name`"),
        "expected a placeholder-name error, got: {err:?}"
    );
}

#[test]
fn unknown_placeholder_fill_is_reported() {
    let dir = sample_project("badfill");
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\nplaceholder = \"Nope\"\ntext = \"hi\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("placeholder `Nope`"),
        "expected a placeholder-fill error, got: {err:?}"
    );
}

#[test]
fn gwen_new_scaffolds_a_buildable_project() {
    let dir = tmp("scaffold");
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("new")
        .arg(dir.as_os_str())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("main.toml").is_file());
    assert!(dir.join("slides").join("title.toml").is_file());

    let deck = gwen::build(&dir).unwrap();
    let bytes = std::fs::read(&deck).unwrap();
    assert_eq!(&bytes[0..2], b"PK");
}
