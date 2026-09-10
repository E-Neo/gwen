//! End-to-end tests: scaffold a project, build it, reopen the package and
//! assert structure and content.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

const TINY_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x00";

fn tmp(name: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "gwen-v2-{}-{}-{}",
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

/// A minimal, complete project.
fn sample_project(name: &str, slides: &[(&str, &str)]) -> PathBuf {
    let dir = tmp(name);
    write(
        &dir.join("config.toml"),
        r##"[presentation]
name = "demo"
slide_width = 12192000
slide_height = 6858000
default_layout = "title"

[theme]
major_font = "Arial Black"
minor_font = "Arial"

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
kind = "shape"
shape = "round_rect"
left = 0
top = 0
width = 12192000
height = 914400
fill = "#C7000A"

[[layouts.content.elements]]
kind = "slot"
slot = "title"
type = "text"
left = 914400
top = 685800
width = 10363200
height = 914400
text_size = 32
color = "#1D1D1A"
bold = true

[[layouts.content.elements]]
kind = "slot"
slot = "body"
type = "text"
left = 914400
top = 1828800
width = 10363200
height = 4114800
text_size = 20
color = "#262626"

[[layouts.picture.elements]]
kind = "slot"
slot = "picture"
type = "picture"
left = 914400
top = 914400
width = 10363200
height = 5029200
"##,
    );
    let mut summary = String::from("# Summary\n\n");
    for (file, _) in slides {
        summary.push_str(&format!("- [{file}](slides/{file}.md)\n"));
    }
    write(&dir.join("src").join("SUMMARY.md"), &summary);
    for (file, body) in slides {
        write(
            &dir.join("src").join("slides").join(format!("{file}.md")),
            body,
        );
    }
    write(&dir.join("src").join("media").join("logo.png"), "");
    std::fs::write(dir.join("src").join("media").join("logo.png"), TINY_PNG).unwrap();
    dir
}

fn build(dir: &Path) -> PathBuf {
    gwen::build(dir).expect("build succeeds")
}

fn zip_entry(path: &Path, name: &str) -> Option<String> {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut entry = zip.by_name(name).ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).unwrap();
    Some(buf)
}

fn zip_has(path: &Path, name: &str) -> bool {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    zip.by_name(name).is_ok()
}

#[test]
fn builds_a_valid_package() {
    let dir = sample_project(
        "basic",
        &[
            ("title", "---\nlayout: title\n---\n\n# Hello\n"),
            (
                "content",
                "---\nlayout: content\nbackground: \"#112233\"\n---\n\n# Points\n\n- one\n- two\n",
            ),
        ],
    );
    let out = build(&dir);
    let pres = zip_entry(&out, "ppt/presentation.xml").unwrap();
    assert!(pres.contains("<p:sldSz cx=\"12192000\" cy=\"6858000\"/>"));
    assert!(pres.contains("<p:sldId id=\"256\""));
    assert!(pres.contains("<p:sldId id=\"257\""));

    let slide1 = zip_entry(&out, "ppt/slides/slide1.xml").unwrap();
    assert!(slide1.contains("<a:t>Hello</a:t>"));
    // Colors are normalized to bare hex.
    assert!(slide1.contains("<a:srgbClr val=\"1D1D1A\"/>"));

    let slide2 = zip_entry(&out, "ppt/slides/slide2.xml").unwrap();
    assert!(slide2.contains("<p:bg>"));
    assert!(slide2.contains("<a:srgbClr val=\"112233\"/>"));
    assert_eq!(slide2.matches("<a:buChar char=\"•\"/>").count(), 2);

    // The decorative layout shape is emitted with its fill.
    let layout = zip_entry(&out, "ppt/slideLayouts/slideLayout1.xml").unwrap();
    assert!(layout.contains("<a:srgbClr val=\"C7000A\"/>"));

    // Master lists both layouts with unique ids and correct r:ids.
    let master = zip_entry(&out, "ppt/slideMasters/slideMaster1.xml").unwrap();
    assert_eq!(master.matches("<p:sldLayoutId ").count(), 3);
    assert!(master.contains("<a:clrMap "));
}

#[test]
fn picture_gets_a_real_relationship() {
    let dir = sample_project(
        "picture",
        &[(
            "pic",
            "---\nlayout: picture\n---\n\n![logo](media/logo.png)\n",
        )],
    );
    let out = build(&dir);
    assert!(zip_has(&out, "ppt/media/logo.png"));

    let slide = zip_entry(&out, "ppt/slides/slide1.xml").unwrap();
    assert!(slide.contains("<a:blip r:embed=\"rId2\"/>"));
    let rels = zip_entry(&out, "ppt/slides/_rels/slide1.xml.rels").unwrap();
    assert!(rels.contains("Id=\"rId2\""));
    assert!(rels.contains("Target=\"../media/logo.png\""));
    assert!(rels.contains("relationships/image"));
}

#[test]
fn notes_generate_a_master_and_wiring() {
    let dir = sample_project(
        "notes",
        &[(
            "talk",
            "---\nlayout: content\n---\n\n# Talk\n\nbody\n\n## Notes\n\nremember this\n",
        )],
    );
    let out = build(&dir);
    assert!(zip_entry(&out, "ppt/notesMasters/notesMaster1.xml").is_some());
    let notes = zip_entry(&out, "ppt/notesSlides/notesSlide1.xml").unwrap();
    assert!(notes.contains("<a:t>remember this</a:t>"));

    let rels = zip_entry(&out, "ppt/notesSlides/_rels/notesSlide1.xml.rels").unwrap();
    assert!(rels.contains("relationships/notesMaster"));
    assert!(rels.contains("relationships/slide"));

    let pres = zip_entry(&out, "ppt/presentation.xml").unwrap();
    assert!(pres.contains("<p:notesMasterIdLst>"));
}

#[test]
fn relative_targets_are_used_everywhere() {
    let dir = sample_project(
        "rels",
        &[
            ("title", "---\nlayout: title\n---\n\n# A\n"),
            ("content", "---\nlayout: content\n---\n\n# B\n"),
        ],
    );
    let out = build(&dir);
    let pres_rels = zip_entry(&out, "ppt/_rels/presentation.xml.rels").unwrap();
    assert!(pres_rels.contains("Target=\"slides/slide1.xml\""));
    assert!(pres_rels.contains("Target=\"slideMasters/slideMaster1.xml\""));
    assert!(pres_rels.contains("Target=\"theme/theme1.xml\""));

    let slide_rels = zip_entry(&out, "ppt/slides/_rels/slide1.xml.rels").unwrap();
    assert!(slide_rels.contains("Target=\"../slideLayouts/slideLayout"));
    // No doubled/absolute paths.
    assert!(!pres_rels.contains("ppt/ppt/"));
}

#[test]
fn content_types_cover_every_part() {
    let dir = sample_project(
        "ct",
        &[
            ("title", "---\nlayout: title\n---\n\n# A\n"),
            (
                "pic",
                "---\nlayout: picture\n---\n\n![logo](media/logo.png)\n",
            ),
        ],
    );
    let out = build(&dir);
    let ct = zip_entry(&out, "[Content_Types].xml").unwrap();
    assert!(ct.contains("Extension=\"png\""));
    assert!(ct.contains("PartName=\"/ppt/slides/slide1.xml\""));
    assert!(ct.contains("PartName=\"/ppt/slideMasters/slideMaster1.xml\""));
    assert!(ct.contains("PartName=\"/ppt/theme/theme1.xml\""));
    assert!(ct.contains("PartName=\"/docProps/core.xml\""));
}

#[test]
fn unknown_slot_is_a_diagnostic() {
    // The title layout has no `body` slot, so a plain paragraph cannot bind.
    let dir = sample_project(
        "badslot",
        &[(
            "title",
            "---\nlayout: title\n---\n\n# A\n\nloose paragraph\n",
        )],
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        err.to_string().contains("no `body` slot"),
        "diagnostic names the missing slot: {err}"
    );
}

#[test]
fn missing_media_is_a_diagnostic() {
    let dir = sample_project(
        "missingmedia",
        &[("pic", "---\nlayout: picture\n---\n\n![x](media/nope.png)\n")],
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        err.to_string().contains("nope.png"),
        "diagnostic names the missing media file: {err}"
    );
}

#[test]
fn output_is_deterministic() {
    let dir = sample_project(
        "deterministic",
        &[
            ("title", "---\nlayout: title\n---\n\n# A\n"),
            ("content", "---\nlayout: content\n---\n\n# B\n\n- x\n"),
        ],
    );
    let first = std::fs::read(build(&dir)).unwrap();
    let second = std::fs::read(build(&dir)).unwrap();
    assert_eq!(first, second, "building twice yields identical bytes");
}

#[test]
fn theme_has_the_required_scheme_shape() {
    let dir = sample_project("theme", &[("title", "---\nlayout: title\n---\n\n# A\n")]);
    let out = build(&dir);
    let theme = zip_entry(&out, "ppt/theme/theme1.xml").unwrap();
    // Exactly three entries in each fmtScheme list (schema requirement):
    // fillStyleLst = 1 solid + 2 gradients, bgFillStyleLst = 1 solid + 2
    // gradients, lnStyleLst = 3 lines, effectStyleLst = 3 effects.
    assert_eq!(theme.matches("<a:gradFill").count(), 4);
    assert_eq!(theme.matches("<a:ln ").count(), 3);
    assert_eq!(theme.matches("<a:effectStyle>").count(), 3);
    assert_eq!(theme.matches("<a:solidFill>").count(), 5);
    // Font faces come from config and include the required ea/cs children.
    assert!(theme.contains("<a:latin typeface=\"Arial Black\"/>"));
    assert!(theme.contains("<a:ea typeface=\"\"/>"));
    assert!(theme.contains("<a:cs typeface=\"\"/>"));
}
