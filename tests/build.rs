//! End-to-end smoke tests: scaffold a TOML project, build it, and verify the
//! output is a real `.pptx` package. Content mapping is covered by unit tests
//! on `render::spec`; here we only assert the packaged artifact is well-formed
//! and that `gwen new` produces a buildable project.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use base64::Engine;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use std::io::{Read, Write};

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

/// A minimal, valid RGB PNG of the given pixel dimensions.
fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    fn chunk(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(12 + data.len());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        let mut body = Vec::with_capacity(4 + data.len());
        body.extend_from_slice(tag);
        body.extend_from_slice(data);
        out.extend_from_slice(&crc32fast::hash(&body).to_be_bytes());
        out
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    let mut raw = Vec::with_capacity(h as usize * (1 + w as usize * 3));
    for _ in 0..h {
        raw.push(0);
        raw.extend(std::iter::repeat_n(0u8, w as usize * 3));
    }
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&raw).unwrap();
    out.extend_from_slice(&chunk(b"IDAT", &enc.finish().unwrap()));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
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

[styles.shape]
fill = { color = "C7000A" }

[styles.text]
font_face = "Arial"
color = "262626"

[styles.named.muted]
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
notes = "remember && escape <!--"

[[shapes]]
type = "text"
x = "0.8in"
y = "1.5in"
w = "7in"
h = "4in"
text = "line one\n\nline two"
styles = "muted"
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
"##,
    );
    std::fs::create_dir_all(dir.join("media")).unwrap();
    std::fs::write(dir.join("media").join("pixel.png"), pixel_png()).unwrap();
    dir
}

/// Extract one member from a zip produced by `gwen::zip`, returning its
/// uncompressed bytes.
fn zip_member(bytes: &[u8], want: &str) -> Option<Vec<u8>> {
    let eocd_at = bytes.windows(4).rposition(|w| w == b"PK\x05\x06")?;
    let cd_offset = u32::from_le_bytes(bytes[eocd_at + 16..eocd_at + 20].try_into().ok()?) as usize;
    let mut pos = cd_offset;
    loop {
        if pos + 46 > bytes.len() || &bytes[pos..pos + 4] != b"PK\x01\x02" {
            return None;
        }
        let name_len = u16::from_le_bytes(bytes[pos + 28..pos + 30].try_into().ok()?) as usize;
        let extra_len = u16::from_le_bytes(bytes[pos + 30..pos + 32].try_into().ok()?) as usize;
        let comment_len = u16::from_le_bytes(bytes[pos + 32..pos + 34].try_into().ok()?) as usize;
        let local_offset = u32::from_le_bytes(bytes[pos + 42..pos + 46].try_into().ok()?) as usize;
        let name = std::str::from_utf8(&bytes[pos + 46..pos + 46 + name_len])
            .ok()?
            .to_string();
        if name == want {
            let lh_name = u16::from_le_bytes(
                bytes[local_offset + 26..local_offset + 28]
                    .try_into()
                    .ok()?,
            ) as usize;
            let lh_extra = u16::from_le_bytes(
                bytes[local_offset + 28..local_offset + 30]
                    .try_into()
                    .ok()?,
            ) as usize;
            let comp_size = u32::from_le_bytes(
                bytes[local_offset + 18..local_offset + 22]
                    .try_into()
                    .ok()?,
            ) as usize;
            let data = &bytes[local_offset + 30 + lh_name + lh_extra
                ..local_offset + 30 + lh_name + lh_extra + comp_size];
            let mut out = Vec::new();
            DeflateDecoder::new(data).read_to_end(&mut out).ok()?;
            return Some(out);
        }
        pos += 46 + name_len + extra_len + comment_len;
    }
}

/// The member names of a zip produced by `gwen::zip`, in central-directory
/// order.
fn zip_names(bytes: &[u8]) -> Vec<String> {
    let Some(eocd_at) = bytes.windows(4).rposition(|w| w == b"PK\x05\x06") else {
        return Vec::new();
    };
    let cd_offset =
        u32::from_le_bytes(bytes[eocd_at + 16..eocd_at + 20].try_into().unwrap()) as usize;
    let mut pos = cd_offset;
    let mut names = Vec::new();
    loop {
        if pos + 46 > bytes.len() || &bytes[pos..pos + 4] != b"PK\x01\x02" {
            return names;
        }
        let name_len = u16::from_le_bytes(bytes[pos + 28..pos + 30].try_into().unwrap()) as usize;
        let extra_len = u16::from_le_bytes(bytes[pos + 30..pos + 32].try_into().unwrap()) as usize;
        let comment_len =
            u16::from_le_bytes(bytes[pos + 32..pos + 34].try_into().unwrap()) as usize;
        let Ok(name) = std::str::from_utf8(&bytes[pos + 46..pos + 46 + name_len]) else {
            return names;
        };
        names.push(name.to_string());
        pos += 46 + name_len + extra_len + comment_len;
    }
}

/// Parse `p14:section` blocks out of `presentation.xml`.
fn sections(xml: &str) -> Vec<(String, String, usize)> {
    fn between(s: &str, open: &str, close: char) -> Option<String> {
        let i = s.find(open)? + open.len();
        let rest = &s[i..];
        let end = rest.find(close)?;
        Some(rest[..end].to_string())
    }
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(rel) = xml[pos..].find("<p14:section ") {
        let start = pos + rel;
        let Some(gap) = xml[start..].find('>') else {
            break;
        };
        let head_end = start + gap;
        let head = &xml[start + "<p14:section ".len()..head_end];
        let Some(gap2) = xml[start..].find("</p14:section>") else {
            break;
        };
        let end = start + gap2;
        out.push((
            between(head, "name=\"", '"').unwrap_or_default(),
            between(head, "id=\"{", '}').unwrap_or_default(),
            xml[start..end].matches("<p14:sldId ").count(),
        ));
        pos = end + "</p14:section>".len();
    }
    out
}

#[test]
fn multiple_sections_have_unique_ids() {
    let dir = sample_project("multisection");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = main.replace(
        "[[sections]]\ntitle = \"Intro\"\nslides = [\"title.toml\", \"content.toml\"]\n",
        "[[sections]]\ntitle = \"Cover\"\nslides = [\"title.toml\"]\n\n[[sections]]\ntitle = \"Content\"\nslides = []\n\n[[sections]]\ntitle = \"End\"\nslides = [\"content.toml\"]\n",
    );
    std::fs::write(dir.join("main.toml"), main).unwrap();

    let out = gwen::build(&dir).unwrap();
    let xml = zip_member(&std::fs::read(&out).unwrap(), "ppt/presentation.xml")
        .expect("presentation.xml in zip");
    let xml = String::from_utf8(xml).unwrap();
    let sections = sections(&xml);

    assert_eq!(
        sections
            .iter()
            .map(|(n, _, _)| n.as_str())
            .collect::<Vec<_>>(),
        vec!["Cover", "Content", "End"],
        "section names/order from multipart presentation.xml"
    );
    let ids: Vec<&str> = sections.iter().map(|(_, id, _)| id.as_str()).collect();
    assert_eq!(ids.len(), 3, "three section ids");
    let unique: std::collections::HashSet<&&str> = ids.iter().collect();
    assert_eq!(unique.len(), 3, "section ids must be unique, got: {ids:?}");
    assert_eq!(
        sections.iter().map(|(_, _, n)| *n).collect::<Vec<_>>(),
        vec![1, 0, 1],
        "Cover and End each hold one slide, Content none"
    );
}

#[test]
fn rect_with_text_renders_bold_inside() {
    let dir = sample_project("recttext");
    write(
        &dir.join("slides").join("content.toml"),
        r##"master = "brand"

[[shapes]]
type = "rect"
x = "1cm"
y = "1cm"
w = "4cm"
h = "1cm"
font_size = 14
align = "left"
valign = "top"
text = "**Bold** and plain"
"##,
    );
    let out = gwen::build(&dir).unwrap();
    let xml =
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").expect("slide2 in zip");
    let xml = String::from_utf8(xml).unwrap();
    assert!(
        xml.contains("prstGeom prst=\"rect\""),
        "rect preset is drawn"
    );
    assert!(xml.contains("<a:t>Bold</a:t>"), "bold run text present");
    assert!(xml.contains("b=\"1\""), "bold applied to the run");
    assert!(!xml.contains("**"), "no literal markdown stars");
}

#[test]
fn custom_geometry_emits_cust_geom_path() {
    let dir = sample_project("custgeom");
    write(
        &dir.join("slides").join("content.toml"),
        r##"master = "brand"

[[shapes]]
type = "cust_geom"
x = "1cm"
y = "1cm"
w = "3.8cm"
h = "1.6cm"
line = { color = "A6A6A6", width = 2 }
points = [
  { x = "0cm", y = "1.6cm" },
  { x = "3.8cm", y = "0cm", curve = { type = "cubic", x1 = "0.985cm", y1 = "0.752cm", x2 = "2.405cm", y2 = "0.119cm" } },
]
"##,
    );
    let out = gwen::build(&dir).unwrap();
    let xml =
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").expect("slide2 in zip");
    let xml = String::from_utf8(xml).unwrap();
    assert!(xml.contains("<a:custGeom>"), "custom geometry emitted");
    assert!(xml.contains("<a:cubicBezTo>"), "cubic segment emitted");
    assert!(
        xml.contains("<a:moveTo>"),
        "path starts with an explicit move"
    );
    assert!(
        !xml.contains("prst=\"custGeom\""),
        "custGeom must not be emitted as a preset geometry"
    );
    assert!(
        !xml.contains("<a:close/>"),
        "an open custom path must not be closed"
    );
}

#[test]
fn closed_custom_geometry_emits_close() {
    let dir = sample_project("custgeomclose");
    write(
        &dir.join("slides").join("content.toml"),
        r##"master = "brand"

[[shapes]]
type = "cust_geom"
x = "1cm"
y = "1cm"
w = "2cm"
h = "1cm"
fill = { color = "CCECFF" }
line = { color = "000000", width = 1 }
points = [
  { x = "0cm", y = "0cm", moveTo = true },
  { x = "2cm", y = "0cm" },
  { x = "2cm", y = "1cm" },
  { close = true },
]
"##,
    );
    let out = gwen::build(&dir).unwrap();
    let xml =
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").expect("slide2 in zip");
    let xml = String::from_utf8(xml).unwrap();
    assert!(xml.contains("<a:custGeom>"), "custom geometry emitted");
    assert!(xml.contains("<a:lnTo>"), "line segments emitted");
    assert!(xml.contains("<a:close/>"), "closed path emitted");
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
    let names = zip_names(&first);
    assert_eq!(names, zip_names(&second), "same part list");
    for name in names {
        // core.xml carries the build timestamp, so compare every other part.
        if name.ends_with('/') || name == "docProps/core.xml" {
            continue;
        }
        assert_eq!(
            zip_member(&first, &name),
            zip_member(&second, &name),
            "part {name} is deterministic"
        );
    }
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
fn unknown_toml_field_is_reported() {
    let dir = sample_project("badfield");
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"brand\"\n\n[[shaps]]\ntype = \"text\"\ntext = \"hi\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("`shaps`"),
        "expected an unknown-field error for `shaps`, got: {msg}"
    );
}

#[test]
fn unknown_pptxgen_option_is_reported() {
    let dir = sample_project("badopt");
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"hi\"\nfount_size = 14\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("fount_size") && msg.contains("unknown text option"),
        "expected an unknown-option error, got: {msg}"
    );
}

#[test]
fn unknown_style_type_is_reported() {
    let dir = sample_project("badstyletype");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = format!("{main}\n[styles.rectt]\nfill = {{ color = \"000000\" }}\n");
    std::fs::write(dir.join("main.toml"), main).unwrap();
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("unknown style type `[styles.rectt]`"),
        "expected a style-type error"
    );
}

#[test]
fn unknown_style_reference_is_reported() {
    let dir = sample_project("badstyleref");
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"hi\"\nstyles = \"nope\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("unknown style `nope`"),
        "expected a style-reference error"
    );
}

#[test]
fn unknown_shape_type_is_reported() {
    let dir = sample_project("badshapetype");
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"rectt\"\ntext = \"hi\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("unknown shape type `rectt`"),
        "expected a shape-type error"
    );
}

#[test]
fn preset_style_bucket_allows_text_options() {
    let dir = sample_project("rectstyle");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main =
        format!("{main}\n[styles.rect]\nfont_face = \"Arial\"\nfill = {{ color = \"112233\" }}\n");
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nh = \"1in\"\n",
    );
    gwen::build(&dir).unwrap();
}

#[test]
fn text_bucket_only_applies_to_the_text_type() {
    let dir = sample_project("textbucket");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = main
        .replace(
            "[styles.shape]\nfill = { color = \"C7000A\" }",
            "[styles.shape]\nfont_face = \"Georgia\"\nfill = { color = \"C7000A\" }",
        )
        .replace(
            "[styles.text]\nfont_face = \"Arial\"\ncolor = \"262626\"",
            "[styles.text]\nfont_face = \"Courier New\"\ncolor = \"FF00FF\"",
        );
    let main = format!("{main}\n[styles.rect]\nfont_size = 14\n");
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nh = \"1in\"\ntext = \"**Bold** text\"\n",
    );
    // A plain text shape, not a placeholder fill: the master placeholder's
    // own font would otherwise win over the slide run.
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\nx = \"0.8in\"\ny = \"1.5in\"\nw = \"7in\"\nh = \"1in\"\ntext = \"Plain text\"\n",
    );

    let out = gwen::build(&dir).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let slide2 =
        String::from_utf8(zip_member(&bytes, "ppt/slides/slide2.xml").expect("slide2 in zip"))
            .unwrap();
    // A preset with text stays a preset: it gets [styles.shape] and
    // [styles.rect], never [styles.text].
    assert!(
        slide2.contains("Georgia"),
        "text options come from [styles.shape]"
    );
    assert!(
        slide2.contains("sz=\"1400\""),
        "font_size from [styles.rect] wins"
    );
    assert!(
        !slide2.contains("Courier New"),
        "[styles.text] must not leak into a rect"
    );
    assert!(
        !slide2.contains("FF00FF"),
        "[styles.text] color must not leak into a rect"
    );
    assert!(slide2.contains("<a:t>Bold</a:t>"), "text rendered");

    // `type = "text"` still gets its own bucket, which wins over
    // [styles.shape].
    let slide1 =
        String::from_utf8(zip_member(&bytes, "ppt/slides/slide1.xml").expect("slide1 in zip"))
            .unwrap();
    assert!(
        slide1.contains("Courier New"),
        "[styles.text] still styles type=text"
    );
}

#[test]
fn unit_string_margins_are_emitted_as_emu() {
    let dir = sample_project("marginunits");
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nh = \"1in\"\ntext = \"Padded\"\nmargin = [\"0.13cm\", \"0.25cm\", \"0.13cm\", \"0.25cm\"]\n",
    );
    let out = gwen::build(&dir).unwrap();
    let xml = String::from_utf8(
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").unwrap(),
    )
    .unwrap();
    assert!(
        xml.contains("lIns=\"90000\" tIns=\"46800\" rIns=\"90000\" bIns=\"46800\""),
        "unit-string margins become EMU with top/left in the documented order: {xml}"
    );
}

#[test]
fn bullet_indent_applies_to_markdown_lists() {
    let dir = sample_project("bulletindent");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = main.replace(
        "[styles.shape]\nfill = { color = \"C7000A\" }",
        "[styles.shape]\nbullet_indent = 10\nfill = { color = \"C7000A\" }",
    );
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\nx = \"0.8in\"\ny = \"1.5in\"\nw = \"7in\"\nh = \"4in\"\ntext = \"- one\\n  - two\"\n",
    );

    let out = gwen::build(&dir).unwrap();
    let xml = String::from_utf8(
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").unwrap(),
    )
    .unwrap();
    assert!(
        xml.contains("marL=\"127000\" indent=\"-127000\""),
        "10pt hanging indent on the top-level item: {xml}"
    );
    assert!(
        xml.contains("marL=\"254000\" indent=\"-127000\""),
        "nested item adds one 10pt step: {xml}"
    );
}

#[test]
fn explicit_bullet_indent_overrides_the_style_default() {
    let dir = sample_project("bulletindentoverride");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = main.replace(
        "[styles.shape]\nfill = { color = \"C7000A\" }",
        "[styles.shape]\nbullet_indent = 10\nfill = { color = \"C7000A\" }",
    );
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\nx = \"0.8in\"\ny = \"1.5in\"\nw = \"7in\"\nh = \"4in\"\n\n[[shapes.paragraphs]]\ntext = \"custom\"\nbullet = { indent = 20 }\n\n[[shapes.paragraphs]]\ntext = \"custom two\"\nbullet = true\nbullet_indent = 30\n",
    );

    let out = gwen::build(&dir).unwrap();
    let xml = String::from_utf8(
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").unwrap(),
    )
    .unwrap();
    assert!(
        xml.contains("marL=\"254000\" indent=\"-254000\""),
        "an explicit bullet.indent wins over bullet_indent: {xml}"
    );
    assert!(
        xml.contains("marL=\"381000\" indent=\"-381000\""),
        "a paragraph bullet_indent also wins over the style default: {xml}"
    );
}

#[test]
fn bare_number_margins_stay_in_points() {
    let dir = sample_project("marginpoints");
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nh = \"1in\"\ntext = \"Padded\"\nmargin = [5, 10, 5, 10]\n",
    );
    let out = gwen::build(&dir).unwrap();
    let xml = String::from_utf8(
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").unwrap(),
    )
    .unwrap();
    assert!(
        xml.contains("lIns=\"127000\" tIns=\"63500\" rIns=\"127000\" bIns=\"63500\""),
        "bare-number margins are pptxgenjs points: {xml}"
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
fn svg_images_embed_vector_and_raster_preview() {
    let dir = tmp("svg-deck");
    write(
        &dir.join("main.toml"),
        r##"[presentation]
title = "svg deck"
width = "13.333in"
height = "7.5in"

[theme]
major_font = "Arial"
minor_font = "Arial"

[[sections]]
title = "Intro"
slides = ["one.toml"]
"##,
    );
    write(
        &dir.join("masters").join("brand.toml"),
        r##"[[shapes]]
type = "placeholder"
ph_type = "title"
name = "Title"
x = "0.8in"
y = "0.25in"
w = "11.7in"
h = "0.66in"

[[shapes]]
type = "image"
x = "11in"
y = "0.1in"
w = "1in"
h = "0.5in"
src = "media/logo.svg"
"##,
    );
    write(
        &dir.join("slides").join("one.toml"),
        r##"master = "brand"

[[shapes]]
type = "text"
placeholder = "Title"
text = "Hello"

[[shapes]]
type = "image"
x = "2in"
y = "2in"
w = "4in"
h = "2in"
src = "media/logo.svg"
"##,
    );
    std::fs::create_dir_all(dir.join("media")).unwrap();
    std::fs::write(
        dir.join("media").join("logo.svg"),
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><rect width="200" height="100" fill="#C7000A"/></svg>"##,
    )
    .unwrap();

    let deck = gwen::build(&dir).unwrap();
    let bytes = std::fs::read(&deck).unwrap();

    // The deck embeds the SVG as a real vector part *and* a raster preview.
    let media: Vec<String> = zip_names(&bytes)
        .into_iter()
        .filter(|n| n.starts_with("ppt/media/"))
        .collect();
    let svg_part = media
        .iter()
        .find(|n| n.ends_with(".svg"))
        .expect("a vector SVG media part");
    let png_part = media
        .iter()
        .find(|n| n.ends_with(".png"))
        .expect("a rasterized PNG preview part");
    let svg_bytes = zip_member(&bytes, svg_part).unwrap();
    assert!(
        String::from_utf8_lossy(&svg_bytes).contains("viewBox=\"0 0 200 100\""),
        "the svg part holds the real vector data"
    );
    let png_bytes = zip_member(&bytes, png_part).unwrap();
    assert_eq!(
        &png_bytes[0..8],
        &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
        "the preview is a real PNG"
    );

    // Slide and master layout pictures reference the SVG natively.
    for (part, label) in [
        ("ppt/slides/slide1.xml", "slide"),
        ("ppt/slideLayouts/slideLayout1.xml", "master layout"),
    ] {
        let xml = String::from_utf8(zip_member(&bytes, part).unwrap()).unwrap();
        assert!(
            xml.contains("<asvg:svgBlip"),
            "{label} references the vector SVG: {xml}"
        );
        assert!(
            xml.contains("<a:blip r:embed="),
            "{label} keeps the PNG fallback"
        );
    }
}

#[test]
fn locked_slide_number_is_baked_and_locked() {
    let dir = tmp("locked-num");
    write(
        &dir.join("main.toml"),
        r##"[presentation]
title = "locked deck"
width = "13.333in"
height = "7.5in"

[theme]
major_font = "Arial"
minor_font = "Arial"

[[sections]]
title = "Intro"
slides = ["one.toml"]
"##,
    );
    write(
        &dir.join("masters").join("brand.toml"),
        r##"slide_number = { x = "12.2in", y = "7.1in", w = "1in", h = "0.3in", font_size = 12, locked = true }
"##,
    );
    write(&dir.join("slides").join("one.toml"), "master = \"brand\"\n");

    let deck = gwen::build(&dir).unwrap();
    let bytes = std::fs::read(&deck).unwrap();
    let xml = String::from_utf8(zip_member(&bytes, "ppt/slides/slide1.xml").unwrap()).unwrap();
    assert!(
        !xml.contains("<p:ph type=\"sldNum\""),
        "placeholder removed so PowerPoint honors the locks: {xml}"
    );
    assert!(
        xml.contains(
            "<a:spLocks noGrp=\"1\" noSelect=\"1\" noMove=\"1\" noResize=\"1\" noTextEdit=\"1\"/>"
        ),
        "shape fully locked: {xml}"
    );
    assert!(
        xml.contains("type=\"slidenum\""),
        "the number still updates per slide: {xml}"
    );
    // The leaked master-level copy is stripped so no other slide inherits it.
    let master =
        String::from_utf8(zip_member(&bytes, "ppt/slideMasters/slideMaster1.xml").unwrap())
            .unwrap();
    assert!(
        !master.contains("Slide Number Placeholder 0"),
        "no leaked number on the shared master: {master}"
    );
}

#[test]
fn slide_numbers_stay_per_master() {
    let dir = tmp("per-master-slnum");
    write(
        &dir.join("main.toml"),
        r##"[presentation]
title = "two masters"
width = "13.333in"
height = "7.5in"

[theme]
major_font = "Arial"
minor_font = "Arial"

[[sections]]
title = "Intro"
slides = ["a.toml", "b.toml"]
"##,
    );
    write(
        &dir.join("masters").join("amaster.toml"),
        r##"slide_number = { x = "1in", y = "1in", w = "1in", h = "0.3in", font_size = 12, locked = true }
"##,
    );
    write(
        &dir.join("masters").join("bmaster.toml"),
        r##"slide_number = { x = "6in", y = "6in", w = "1in", h = "0.3in", font_size = 24, color = "FF0000" }
"##,
    );
    write(&dir.join("slides").join("a.toml"), "master = \"amaster\"\n");
    write(&dir.join("slides").join("b.toml"), "master = \"bmaster\"\n");

    let deck = gwen::build(&dir).unwrap();
    let bytes = std::fs::read(&deck).unwrap();

    // Master A's slide: its own geometry + font, and locked.
    let a = String::from_utf8(zip_member(&bytes, "ppt/slides/slide1.xml").unwrap()).unwrap();
    let a_num = &a[a.find("Slide Number Placeholder").unwrap()..];
    assert!(
        a_num.contains("<a:off x=\"914400\" y=\"914400\""),
        "A's x/y: {a_num}"
    );
    assert!(a_num.contains("sz=\"1200\""), "A's font size: {a_num}");
    assert!(
        !a.contains("<p:ph type=\"sldNum\""),
        "A's number is a plain locked shape, not a placeholder: {a_num}"
    );
    assert!(
        a_num.contains("noTextEdit=\"1\""),
        "A is locked because A is locked: {a_num}"
    );
    // Master A's layout stays editable in Slide Master view.
    let a_layout =
        String::from_utf8(zip_member(&bytes, "ppt/slideLayouts/slideLayout1.xml").unwrap())
            .unwrap();
    assert!(
        !a_layout.contains("noTextEdit"),
        "A's layout is not locked (editable in master view): {a_layout}"
    );
    // Master B's slide: its own geometry + font + color, not locked.
    let b = String::from_utf8(zip_member(&bytes, "ppt/slides/slide2.xml").unwrap()).unwrap();
    let b_num = &b[b.find("Slide Number Placeholder").unwrap()..];
    assert!(
        b_num.contains("<a:off x=\"5486400\" y=\"5486400\""),
        "B's x/y: {b_num}"
    );
    assert!(b_num.contains("sz=\"2400\""), "B's font size: {b_num}");
    assert!(!b_num.contains("noTextEdit"), "B is not locked: {b_num}");
    // The shared master leaks neither master's style.
    let master =
        String::from_utf8(zip_member(&bytes, "ppt/slideMasters/slideMaster1.xml").unwrap())
            .unwrap();
    assert!(
        !master.contains("Slide Number Placeholder 0"),
        "no number on the shared master: {master}"
    );
}

#[test]
fn slide_number_stays_a_placeholder_by_default() {
    let dir = sample_project("slnum-placeholder");
    let deck = gwen::build(&dir).unwrap();
    let bytes = std::fs::read(&deck).unwrap();
    let xml = String::from_utf8(zip_member(&bytes, "ppt/slides/slide1.xml").unwrap()).unwrap();
    assert!(
        xml.contains("<p:ph type=\"sldNum\""),
        "default slide_number keeps the editable placeholder: {xml}"
    );
    assert!(
        !xml.contains("noTextEdit"),
        "no lock applied without `locked = true`: {xml}"
    );
}

#[test]
fn base_shape_style_text_keys_do_not_break_images() {
    let dir = tmp("shape-style-image");
    write(
        &dir.join("main.toml"),
        r##"[presentation]
title = "shape style"
width = "13.333in"
height = "7.5in"

[theme]
major_font = "Arial"
minor_font = "Arial"

[styles.shape]
align = "left"
bold = true

[[sections]]
title = "Intro"
slides = ["one.toml"]
"##,
    );
    write(
        &dir.join("masters").join("brand.toml"),
        r##"[[shapes]]
type = "image"
x = "1in"
y = "1in"
w = "1in"
h = "1in"
src = "media/pixel.png"
"##,
    );
    write(
        &dir.join("slides").join("one.toml"),
        r##"master = "brand"

[[shapes]]
type = "image"
x = "2in"
y = "2in"
w = "1in"
h = "1in"
src = "media/pixel.png"

[[shapes]]
type = "text"
text = "hello"
"##,
    );
    std::fs::create_dir_all(dir.join("media")).unwrap();
    std::fs::write(dir.join("media").join("pixel.png"), pixel_png()).unwrap();

    // Text-only base-style keys are ignored by image shapes, not errors.
    let deck = gwen::build(&dir).unwrap();
    let bytes = std::fs::read(&deck).unwrap();
    let slide = String::from_utf8(zip_member(&bytes, "ppt/slides/slide1.xml").unwrap()).unwrap();
    // The image pic gets no `align`; the text shape still carries it.
    let pic = &slide[slide.find("<p:pic>").unwrap()..slide.find("</p:pic>").unwrap()];
    assert!(!pic.contains("algn="), "no alignment on an image: {pic}");
    assert!(slide.contains("algn=\"l\""), "text alignment still applied");
}

#[test]
fn base_shape_style_typos_still_fail() {
    let dir = sample_project("style-typo");
    write(
        &dir.join("main.toml"),
        r##"[presentation]
title = "typo"
width = "13.333in"
height = "7.5in"

[theme]
major_font = "Arial"
minor_font = "Arial"

[styles.shape]
alignment = "left"

[[sections]]
title = "Intro"
slides = ["title.toml"]
"##,
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("unknown style option `alignment`"),
        "bucket typos stay errors: {err:?}"
    );
}

#[test]
fn gwen_render_writes_stem_named_pngs() {
    let dir = sample_project("render-png");
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("render")
        .arg(dir.as_os_str())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let target = dir.join("target").join("render");
    for stem in ["title", "content"] {
        let png = std::fs::read(target.join(format!("{stem}.png"))).unwrap();
        assert_eq!(
            &png[..8],
            &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
            "{stem} is a PNG"
        );
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert!(
            (2500..=2600).contains(&width) && (1400..=1500).contains(&height),
            "{stem} rasters at 2x: {width}x{height}"
        );
        assert!(
            !target.join(format!("{stem}.svg")).exists(),
            "PNG is the default; no SVG is written"
        );
    }
    let mut files: Vec<String> = std::fs::read_dir(&target)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(
        files,
        ["content.png".to_string(), "title.png".to_string()],
        "only stem-named images are written; no preview.json or legacy slide-NN files"
    );
}

#[test]
fn gwen_render_svg_pages_and_clears_stale_outputs() {
    let dir = sample_project("render-svg");
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_gwen"))
            .arg("render")
            .arg(dir.as_os_str())
            .args(extra)
            .output()
            .unwrap()
    };
    let first = run(&[]);
    assert!(first.status.success());
    let target = dir.join("target").join("render");
    assert!(target.join("title.png").exists());
    assert!(target.join("content.png").exists());

    let svg = run(&["--format", "svg", "--page", "2"]);
    assert!(
        svg.status.success(),
        "svg render failed: {}",
        String::from_utf8_lossy(&svg.stderr)
    );
    let second = std::fs::read_to_string(target.join("content.svg")).unwrap();
    assert!(second.contains("line one"), "slide text is painted");
    assert!(second.contains("line two"));
    assert!(
        second.contains("fill=\"#112233\""),
        "slide background is painted"
    );
    assert!(!target.join("title.svg").exists(), "only page 2 selected");
    assert!(
        !target.join("title.png").exists(),
        "stale PNG from the earlier run is removed"
    );
    assert!(
        !target.join("content.png").exists(),
        "only requested formats are written"
    );
    assert!(!target.join("preview.json").exists());

    let bad = run(&["--format", "jpg"]);
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("unknown render format"),
        "format error: {}",
        String::from_utf8_lossy(&bad.stderr)
    );
}

#[test]
fn gwen_render_grows_resized_text_shapes_and_emits_filter_ids() {
    let dir = sample_project("render-autofit");
    let slide = dir.join("slides").join("content.toml");
    let mut toml = std::fs::read_to_string(&slide).unwrap();
    toml.push_str(
        r##"
[[shapes]]
type = "rect"
x = "8.5in"
y = "3.5in"
w = "3in"
h = "0.25in"
fill = { color = "00FF00" }
shadow = { type = "outer", color = "000000", opacity = 0.5, blur = 4, offset = 2, angle = 45 }
text = "This box is far too short for all of this text"
font_size = 16
fit = "resize"
"##,
    );
    std::fs::write(&slide, toml).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("render")
        .arg(dir.as_os_str())
        .args(["--format", "svg", "--page", "2"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let svg = std::fs::read_to_string(dir.join("target/render/content.svg")).unwrap();
    assert!(svg.contains("<filter id=\"fx"), "filter has an id");
    assert!(
        svg.contains("filter=\"url(#fx"),
        "shape references the filter"
    );

    let path = svg
        .split("<path")
        .find(|tag| tag.contains("fill=\"#00ff00\""))
        .expect("resized shape is painted");
    let d = path
        .split("d=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let numbers: Vec<f64> = d
        .split_whitespace()
        .filter_map(|part| part.parse().ok())
        .collect();
    assert!(
        numbers[5] > 0.25,
        "resize grew the backing shape beyond its 0.25in start: {d}"
    );
}

#[test]
fn gwen_render_reports_approximations_on_stderr() {
    let dir = sample_project("render-approx");
    let slide = dir.join("slides").join("content.toml");
    let mut toml = std::fs::read_to_string(&slide).unwrap();
    toml.push_str(
        r##"
[[shapes]]
type = "rect"
x = "9in"
y = "4in"
w = "2in"
h = "1in"
fill = { color = "C7000A" }
shadow = { type = "inner", color = "000000", opacity = 0.5, blur = 4 }

[[shapes]]
type = "text"
x = "9in"
y = "2in"
w = "3in"
h = "1in"
text = "double underline"
underline = "dbl"
font_size = 20
"##,
    );
    std::fs::write(&slide, toml).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("render")
        .arg(dir.as_os_str())
        .args(["--format", "svg", "--page", "2"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("inner shadows are approximated"),
        "approximation is reported on stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("underline style `dbl` drawn as a single line"),
        "text approximation is reported on stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        !dir.join("target/render/preview.json").exists(),
        "approximations are reported on stderr, not in a report file"
    );
}

#[test]
fn gwen_new_scaffolds_a_buildable_project() {
    let dir = tmp("scaffold");
    let _ = std::fs::remove_dir_all(&dir);
    let empty_home = tmp("empty-home");
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("new")
        .arg(dir.as_os_str())
        .env("GWEN_HOME", &empty_home)
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

    // `gwen new` writes a project-level skill with valid front-matter.
    let skill_dir = dir.join(".agents").join("skills").join("gwen");
    let skill = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert!(skill.starts_with("---\n"), "SKILL.md has front-matter");
    assert!(skill.contains("name: gwen"), "SKILL.md has the gwen name");
    assert!(skill.contains("description:"), "SKILL.md has a description");
    assert!(
        skill.contains("## Quick rules"),
        "SKILL.md has the quick rules"
    );
    assert!(
        skill.contains("references/toml-dsl.md"),
        "SKILL.md points at the DSL spec"
    );
    // The inventory is auto-generated from the library whitelists.
    let spec = std::fs::read_to_string(skill_dir.join("references").join("toml-dsl.md")).unwrap();
    assert!(
        spec.contains("## Supported shapes and options"),
        "shape section"
    );
    assert!(spec.contains("round_rect"), "preset snake form listed");
    assert!(spec.contains("star5"), "preset listed");
    assert!(spec.contains("font_face"), "text option listed");
    assert!(spec.contains("- **Nested objects**"), "nested option group");
    assert!(spec.contains("`sizing`"), "image option listed");
    assert!(!dir.join("SKILL.md").exists(), "no root SKILL.md anymore");
    // `gwen new` writes a .gitignore ignoring target/.
    assert_eq!(
        std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
        "target/\n",
        "builtin scaffold writes .gitignore"
    );

    // No em dashes (U+2014) anywhere gwen generates.
    for rel in [
        "main.toml",
        "masters/base.toml",
        "slides/title.toml",
        "slides/intro.toml",
        ".agents/skills/gwen/SKILL.md",
        ".agents/skills/gwen/references/toml-dsl.md",
    ] {
        let bytes = std::fs::read(dir.join(rel)).unwrap();
        assert!(
            !bytes.windows(3).any(|w| w == b"\xe2\x80\x94"),
            "em dash in {rel}"
        );
    }
}

/// A fake gwen home containing a `template/` tree.
fn template_home(name: &str) -> PathBuf {
    let home = tmp(name);
    let t = home.join("template");
    write(
        &t.join("main.toml"),
        "[presentation]\ntitle = \"Ignored\"\n\n[[sections]]\ntitle = \"T\"\nslides = [\"a.toml\"]\n",
    );
    write(
        &t.join("masters").join("a.toml"),
        "background = { color = \"FFFFFF\" }\n",
    );
    write(
        &t.join("slides").join("a.toml"),
        "master = \"a\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"from template\"\n",
    );
    write(&t.join("media").join("keep.txt"), "payload\n");
    home
}

#[test]
fn gwen_new_uses_template() {
    let home = template_home("home-t");
    let dir = tmp("from-template");
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("new")
        .arg(dir.as_os_str())
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        main.contains(&format!("title = \"{name}\"")),
        "title overridden"
    );
    assert!(
        !main.contains("title = \"Ignored\""),
        "template title ignored"
    );
    assert!(
        dir.join("masters").join("a.toml").is_file(),
        "masters copied"
    );
    assert!(dir.join("slides").join("a.toml").is_file(), "slides copied");
    assert_eq!(
        std::fs::read_to_string(dir.join("media").join("keep.txt")).unwrap(),
        "payload\n"
    );
    assert!(
        !dir.join("slides").join("title.toml").is_file(),
        "no built-in slide"
    );
    gwen::build(&dir).unwrap();
    // Template without a skill falls back to the default project skill.
    let skill_dir = dir.join(".agents").join("skills").join("gwen");
    let skill = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
    assert!(skill.starts_with("---\n") && skill.contains("name: gwen"));
    assert!(skill_dir.join("references").join("toml-dsl.md").is_file());
}

/// A template home for the `gwen update` tests: `main.toml` with comments,
/// two style tables, a master, a slide and a media file.
fn update_home(name: &str) -> PathBuf {
    let home = tmp(name);
    let template = home.join("template");
    write(
        &template.join("main.toml"),
        r##"[presentation]
title = "Ignored"

[[sections]]
title = "T"
slides = ["a.toml"]

# The type theme.
[theme]
major_font = "Arial Black"
minor_font = "Arial"

# Base shape style.
[styles.shape]
align = "left"

# The card body.
[styles.named.card]
font_size = 12
color = "000000"
"##,
    );
    write(
        &template.join("masters").join("a.toml"),
        "background = { color = \"FFFFFF\" }\n",
    );
    write(
        &template.join("slides").join("a.toml"),
        "master = \"a\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"from template\"\n",
    );
    write(&template.join("media").join("keep.txt"), "payload\n");
    home
}

fn gwen_in(home: &Path, args: &[&std::ffi::OsStr]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gwen"));
    command.env("GWEN_HOME", home);
    for arg in args {
        command.arg(arg);
    }
    command.output().unwrap()
}

/// `gwen update` refreshes template-owned files and merges `[theme]` /
/// `[styles.*]` into `main.toml`, keeping comments, the title and the
/// project's own styles. Slides are never touched.
#[test]
fn gwen_update_refreshes_template_files_and_keeps_project_content() {
    let home = update_home("home-update");
    let template = home.join("template");
    let dir = tmp("update-project");
    let _ = std::fs::remove_dir_all(&dir);
    let out = gwen_in(&home, &[std::ffi::OsStr::new("new"), dir.as_os_str()]);
    assert!(
        out.status.success(),
        "gwen new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The project adds a title of its own, a comment, a project-only key and
    // a named style.
    let main_path = dir.join("main.toml");
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    let main = std::fs::read_to_string(&main_path).unwrap();
    let main = main
        .replace(
            "# Base shape style.\n",
            "# Base shape style.\n# Project: margins are handled per shape here.\n",
        )
        .replace(
            "[styles.shape]\nalign = \"left\"\n",
            "[styles.shape]\nalign = \"left\"\nmargin = 0\n",
        );
    let main = format!("{main}\n# Our own accent.\n[styles.named.custom]\ncolor = \"C7000A\"\n");
    std::fs::write(&main_path, &main).unwrap();
    let slide_path = dir.join("slides").join("a.toml");
    std::fs::write(
        &slide_path,
        "master = \"a\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"project words\"\n",
    )
    .unwrap();

    // The template moves on: a master changes, new files appear, a style key
    // changes and a new style table is added.
    write(
        &template.join("masters").join("a.toml"),
        "background = { color = \"F2F2F2\" }\n",
    );
    write(
        &template.join("masters").join("b.toml"),
        "background = { color = \"EEEEEE\" }\n",
    );
    write(&template.join("media").join("extra.txt"), "extra\n");
    let tpl_main = std::fs::read_to_string(template.join("main.toml")).unwrap();
    let tpl_main = tpl_main
        .replace("font_size = 12\n", "font_size = 13\nbold = true\n")
        .to_string()
        + "\n# The frame style is new.\n[styles.named.frame]\nline = { color = \"D3D3D3\", width = 1 }\n";
    std::fs::write(template.join("main.toml"), tpl_main).unwrap();

    let out = gwen_in(&home, &[std::ffi::OsStr::new("update"), dir.as_os_str()]);
    assert!(
        out.status.success(),
        "gwen update failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("updated"), "reports updates: {stderr}");
    assert!(stderr.contains("created"), "reports creations: {stderr}");

    assert_eq!(
        std::fs::read_to_string(dir.join("masters").join("a.toml")).unwrap(),
        "background = { color = \"F2F2F2\" }\n",
        "template master overwritten"
    );
    assert!(
        dir.join("masters").join("b.toml").is_file(),
        "new master created"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("media").join("extra.txt")).unwrap(),
        "extra\n",
        "new media copied"
    );
    assert_eq!(
        std::fs::read_to_string(&slide_path).unwrap(),
        "master = \"a\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"project words\"\n",
        "slides are never touched"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("target/update-backup/masters/a.toml")).unwrap(),
        "background = { color = \"FFFFFF\" }\n",
        "the replaced file is backed up"
    );

    let updated = std::fs::read_to_string(&main_path).unwrap();
    assert!(
        updated.contains(&format!("title = \"{name}\"")),
        "title kept"
    );
    assert!(updated.contains("[[sections]]"), "sections kept");
    assert!(
        updated.contains("# The type theme."),
        "template comments kept: {updated}"
    );
    assert!(
        updated.contains("# Project: margins are handled per shape here."),
        "project comments kept: {updated}"
    );
    assert!(
        updated.contains("# Our own accent.") && updated.contains("[styles.named.custom]"),
        "project-only style kept: {updated}"
    );
    assert!(updated.contains("margin = 0"), "project-only key kept");
    assert!(updated.contains("font_size = 13"), "template value wins");
    assert!(updated.contains("bold = true"), "template key added");
    assert!(
        updated.contains("# The frame style is new.") && updated.contains("[styles.named.frame]"),
        "new template style added with its comment: {updated}"
    );

    // A second run is a no-op.
    let before = std::fs::read_to_string(&main_path).unwrap();
    let out = gwen_in(&home, &[std::ffi::OsStr::new("update"), dir.as_os_str()]);
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("already up to date"),
        "second update is a no-op: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read_to_string(&main_path).unwrap(), before);
}

#[test]
fn gwen_update_dry_run_writes_nothing() {
    let home = update_home("home-update-dry");
    let template = home.join("template");
    let dir = tmp("update-dry");
    let _ = std::fs::remove_dir_all(&dir);
    write(
        &dir.join("main.toml"),
        "[presentation]\ntitle = \"P\"\n\n[[sections]]\nslides = []\n",
    );
    write(
        &dir.join("masters").join("a.toml"),
        "background = \"old\"\n",
    );
    write(
        &template.join("masters").join("a.toml"),
        "background = \"new\"\n",
    );

    let out = gwen_in(
        &home,
        &[
            std::ffi::OsStr::new("update"),
            std::ffi::OsStr::new("--dry-run"),
            dir.as_os_str(),
        ],
    );
    assert!(out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("would update"),
        "dry run says what: {stderr}"
    );
    assert!(
        stderr.contains("nothing written"),
        "dry run footer: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("masters").join("a.toml")).unwrap(),
        "background = \"old\"\n",
        "dry run writes nothing"
    );
    assert!(
        !dir.join("target").join("update-backup").exists(),
        "dry run writes no backup"
    );
}

#[test]
fn gwen_update_rejects_broken_main_before_writing() {
    let home = update_home("home-update-broken");
    let template = home.join("template");
    let dir = tmp("update-broken");
    let _ = std::fs::remove_dir_all(&dir);
    write(
        &dir.join("main.toml"),
        "[presentation]\ntitle = \"P\"\ntitle = \"Q\"\n\n[[sections]]\nslides = []\n",
    );
    write(
        &dir.join("masters").join("a.toml"),
        "background = \"old\"\n",
    );
    write(
        &template.join("masters").join("a.toml"),
        "background = \"new\"\n",
    );

    let out = gwen_in(&home, &[std::ffi::OsStr::new("update"), dir.as_os_str()]);
    assert!(!out.status.success(), "an invalid main.toml is an error");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("main.toml"),
        "stderr names the file: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("masters").join("a.toml")).unwrap(),
        "background = \"old\"\n",
        "nothing is written when main.toml cannot be merged"
    );
}

#[test]
fn gwen_update_keep_styles_leaves_main_toml_alone() {
    let home = update_home("home-update-styles");
    let dir = tmp("update-styles");
    let _ = std::fs::remove_dir_all(&dir);
    write(
        &dir.join("main.toml"),
        "[presentation]\ntitle = \"P\"\n\n[[sections]]\nslides = []\n",
    );
    let main_path = dir.join("main.toml");
    let before = std::fs::read_to_string(&main_path).unwrap();

    let out = gwen_in(
        &home,
        &[
            std::ffi::OsStr::new("update"),
            std::ffi::OsStr::new("--keep-styles"),
            dir.as_os_str(),
        ],
    );
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(&main_path).unwrap(),
        before,
        "`--keep-styles` leaves main.toml alone"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("masters").join("a.toml")).unwrap(),
        "background = { color = \"FFFFFF\" }\n",
        "template files are still synced"
    );
}

#[test]
fn gwen_update_without_template_errors() {
    let home = tmp("home-update-none");
    let dir = tmp("update-none");
    let _ = std::fs::remove_dir_all(&dir);
    write(
        &dir.join("main.toml"),
        "[presentation]\ntitle = \"P\"\n\n[[sections]]\nslides = []\n",
    );

    let out = gwen_in(&home, &[std::ffi::OsStr::new("update"), dir.as_os_str()]);
    assert!(!out.status.success(), "no template is an error");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("template"),
        "stderr mentions the template: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A project with deliberately messy TOML for the `gwen fmt` tests. The
/// `target/` file must never be touched.
fn fmt_project(name: &str) -> PathBuf {
    let dir = tmp(name);
    write(
        &dir.join("main.toml"),
        "\n\n[ presentation ]\ntitle=\"Fmt\"\n\n\n\n[[sections]]\ntitle=\"S\"\nslides=[\"s.toml\"]\n\n[theme]\nmajor_font=\"Arial\"   # heading\n",
    );
    write(
        &dir.join("masters").join("m.toml"),
        "background={color=\"FFFFFF\"}\n",
    );
    write(
        &dir.join("slides").join("s.toml"),
        "master=\"m\"\n\n[[shapes]]\ntype=\"text\"\ntext=\"  keep  \"\n\n# note\n[[shapes]]\ntype=\"rect\"\nmargin=[1,2,3,4]\n",
    );
    write(
        &dir.join(".agents")
            .join("skills")
            .join("refs")
            .join("r.toml"),
        "a=1\n",
    );
    write(&dir.join("target").join("ignored.toml"), "a=1\n");
    dir
}

/// `gwen fmt` rewrites every project TOML (including `.agents/`), leaves
/// `target/` alone, and the project still builds.
#[test]
fn gwen_fmt_rewrites_project_toml() {
    let home = tmp("home-fmt");
    let dir = fmt_project("fmt-project");

    let out = gwen_in(&home, &[std::ffi::OsStr::new("fmt"), dir.as_os_str()]);
    assert!(
        out.status.success(),
        "gwen fmt failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("4 formatted, 0 unchanged"), "{stderr}");

    assert_eq!(
        std::fs::read_to_string(dir.join("main.toml")).unwrap(),
        "[presentation]\ntitle = \"Fmt\"\n\n[[sections]]\ntitle = \"S\"\nslides = [\"s.toml\"]\n\n[theme]\nmajor_font = \"Arial\" # heading\n",
        "main.toml is normalized"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("masters").join("m.toml")).unwrap(),
        "background = { color = \"FFFFFF\" }\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("slides").join("s.toml")).unwrap(),
        "master = \"m\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"  keep  \"\n\n# note\n[[shapes]]\ntype = \"rect\"\nmargin = [1, 2, 3, 4]\n",
        "comments and string contents survive"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join(".agents/skills/refs/r.toml")).unwrap(),
        "a = 1\n",
        ".agents toml is formatted too"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("target/ignored.toml")).unwrap(),
        "a=1\n",
        "target/ is skipped"
    );

    gwen::build(&dir).expect("the formatted project still builds");

    let out = gwen_in(&home, &[std::ffi::OsStr::new("fmt"), dir.as_os_str()]);
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("all 4 files already formatted"),
        "a second run is a no-op: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn gwen_fmt_check_reports_without_writing() {
    let home = tmp("home-fmt-check");
    let dir = fmt_project("fmt-check");
    let main = dir.join("main.toml");
    let before = std::fs::read_to_string(&main).unwrap();

    let out = gwen_in(
        &home,
        &[
            std::ffi::OsStr::new("fmt"),
            std::ffi::OsStr::new("--check"),
            dir.as_os_str(),
        ],
    );
    assert!(
        !out.status.success(),
        "`--check` exits 1 when work is needed"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("needs format"), "{stderr}");
    assert!(stderr.contains("4 of 4 files need formatting"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(&main).unwrap(),
        before,
        "`--check` writes nothing"
    );

    let out = gwen_in(&home, &[std::ffi::OsStr::new("fmt"), dir.as_os_str()]);
    assert!(out.status.success());

    let out = gwen_in(
        &home,
        &[
            std::ffi::OsStr::new("fmt"),
            std::ffi::OsStr::new("--check"),
            dir.as_os_str(),
        ],
    );
    assert!(out.status.success(), "a clean project passes `--check`");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("all 4 files already formatted"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn gwen_fmt_reports_invalid_toml_and_keeps_going() {
    let home = tmp("home-fmt-bad");
    let dir = fmt_project("fmt-bad");
    let broken = dir.join("slides").join("broken.toml");
    write(
        &broken,
        "master = \"m\"\ntitle = \"one\"\ntitle = \"two\"\n",
    );
    let broken_before = std::fs::read_to_string(&broken).unwrap();

    let out = gwen_in(&home, &[std::ffi::OsStr::new("fmt"), dir.as_os_str()]);
    assert!(!out.status.success(), "a broken file makes the run fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("slides/broken.toml"),
        "stderr names the file: {stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(&broken).unwrap(),
        broken_before,
        "the broken file is left alone"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("main.toml")).unwrap(),
        "[presentation]\ntitle = \"Fmt\"\n\n[[sections]]\ntitle = \"S\"\nslides = [\"s.toml\"]\n\n[theme]\nmajor_font = \"Arial\" # heading\n",
        "other files are still formatted"
    );
}

#[test]
fn gwen_fmt_requires_a_project() {
    let home = tmp("home-fmt-none");
    let dir = tmp("fmt-none");
    write(&dir.join("slides").join("s.toml"), "a=1\n");

    let out = gwen_in(&home, &[std::ffi::OsStr::new("fmt"), dir.as_os_str()]);
    assert!(!out.status.success(), "no main.toml is an error");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("main.toml"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("slides").join("s.toml")).unwrap(),
        "a=1\n",
        "nothing is written"
    );
}

/// A template may ship its own `.agents/skills/*`; they are copied verbatim
/// alongside the generated `gwen` skill.
#[test]
fn template_extra_skill_is_kept() {
    let home = template_home("home-skill");
    write(
        &home
            .join("template")
            .join(".agents")
            .join("skills")
            .join("foo")
            .join("SKILL.md"),
        "---\nname: foo\ndescription: A custom skill\n---\n# foo\n\nkeep me\n",
    );
    let dir = tmp("skill-template");
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("new")
        .arg(dir.as_os_str())
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.join(".agents")
            .join("skills")
            .join("foo")
            .join("SKILL.md")
            .is_file()
    );
    assert!(
        dir.join(".agents")
            .join("skills")
            .join("gwen")
            .join("SKILL.md")
            .is_file()
    );
}

/// The `gwen` skill name is reserved; a template shipping one is an error.
#[test]
fn template_gwen_skill_is_reserved() {
    let home = template_home("home-reserved");
    write(
        &home
            .join("template")
            .join(".agents")
            .join("skills")
            .join("gwen")
            .join("SKILL.md"),
        "---\nname: gwen\ndescription: nope\n---\n",
    );
    let dir = tmp("skill-reserved");
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("new")
        .arg(dir.as_os_str())
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(!out.status.success(), "gwen new should fail");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("reserved"),
        "stderr should mention the reserved name"
    );
}

#[test]
fn gwen_new_falls_back_when_no_template() {
    let home = tmp("home-none");
    let dir = tmp("fallback");
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("new")
        .arg(dir.as_os_str())
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.join("masters").join("base.toml").is_file(),
        "built-in master"
    );
    assert!(
        dir.join("slides").join("intro.toml").is_file(),
        "built-in slides"
    );
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(main.contains(&format!("title = \"{name}\"")));
    let skill = std::fs::read_to_string(
        dir.join(".agents")
            .join("skills")
            .join("gwen")
            .join("SKILL.md"),
    )
    .unwrap();
    assert!(skill.starts_with("---\n") && skill.contains("name: gwen"));
}

#[test]
fn gwen_new_no_template_flag_ignores_template() {
    let home = template_home("home-flag");
    let dir = tmp("no-template");
    let _ = std::fs::remove_dir_all(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("new")
        .arg("--no-template")
        .arg(dir.as_os_str())
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.join("masters").join("base.toml").is_file(),
        "built-in scaffold used"
    );
    assert!(
        !dir.join("slides").join("a.toml").is_file(),
        "template slide absent"
    );
}

#[test]
fn markdown_lists_render_bullets_with_per_level_markers() {
    let dir = sample_project("listmarkers");
    write(
        &dir.join("main.toml"),
        r##"[presentation]
title = "deck"
width = "13.333in"
height = "7.5in"

[theme]
major_font = "Arial"
minor_font = "Arial"

[styles.text]
font_face = "Arial"
ordered_markers = ["A."]
unordered_markers = ["\u25BA"]

[[sections]]
title = "Intro"
slides = ["content.toml"]
"##,
    );
    write(
        &dir.join("slides").join("content.toml"),
        r##"master = "brand"

[[shapes]]
type = "text"
x = "1in"
y = "1in"
w = "8in"
h = "4in"
text = "abc\n- a\n- b\n  1. x\n  2. y"
"##,
    );
    let out = gwen::build(&dir).unwrap();
    let xml =
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide1.xml").expect("slide1 in zip");
    let xml = String::from_utf8(xml).unwrap();
    assert!(
        xml.contains("buChar char=\"&#x25BA;\""),
        "unordered rune bullet"
    );
    assert!(
        xml.contains("buAutoNum type=\"alphaUcPeriod\""),
        "ordered marker \"A.\" maps to alphaUcPeriod"
    );
    assert!(xml.contains("<a:t>abc</a:t>"), "plain line still renders");
    assert!(xml.contains("<a:t>y</a:t>"), "nested item renders");
}

#[test]
fn snake_case_shape_type_builds_as_preset() {
    let dir = sample_project("snakepreset");
    write(
        &dir.join("slides").join("content.toml"),
        r##"master = "brand"

[[shapes]]
type = "round_rect"
x = "1in"
y = "1in"
w = "3in"
h = "1in"
text = "**Box**"
"##,
    );
    let out = gwen::build(&dir).unwrap();
    let xml =
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").expect("slide2 in zip");
    let xml = String::from_utf8(xml).unwrap();
    assert!(
        xml.contains("prstGeom prst=\"roundRect\""),
        "snake preset canonicalised"
    );
    assert!(xml.contains("<a:t>Box</a:t>"), "text inside renders");
}

#[test]
fn snake_case_preset_renders_canonical_prst() {
    let dir = sample_project("snakeprst");
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"round_rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\n",
    );
    let out = gwen::build(&dir).unwrap();
    let xml = zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").unwrap();
    let xml = String::from_utf8(xml).unwrap();
    assert!(
        xml.contains("prst=\"roundRect\""),
        "canonical preset in XML"
    );
    assert!(!xml.contains("round_rect"), "no snake preset leaks");
}

#[test]
fn snake_case_style_bucket_applies() {
    let dir = sample_project("snakebucket");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = format!("{main}\n[styles.round_rect]\nfill = {{ color = \"112233\" }}\n");
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"round_rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\n",
    );
    let out = gwen::build(&dir).unwrap();
    let xml = zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").unwrap();
    let xml = String::from_utf8(xml).unwrap();
    assert!(xml.contains("112233"), "styles.round_rect fill applied");
    assert!(xml.contains("prst=\"roundRect\""));
}

#[test]
fn camel_case_style_bucket_still_applies() {
    let dir = sample_project("camelbucket");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = format!("{main}\n[styles.roundRect]\nfill = {{ color = \"445566\" }}\n");
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"round_rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\n",
    );
    let out = gwen::build(&dir).unwrap();
    let xml = zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").unwrap();
    let xml = String::from_utf8(xml).unwrap();
    assert!(xml.contains("445566"), "styles.roundRect fill applied");
}

#[test]
fn master_snake_preset_spec_type_is_canonical() {
    let dir = sample_project("mastersnake");
    write(
        &dir.join("masters").join("brand.toml"),
        "[[shapes]]\ntype = \"round_rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\n",
    );
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"hi\"\n",
    );
    let project = gwen::model::Project::load(&dir).unwrap();
    let spec: serde_json::Value =
        serde_json::from_str(&gwen::render::spec_json(&project).unwrap()).unwrap();
    let masters = spec["masters"].as_array().unwrap();
    let obj = &masters[0]["objects"][0];
    assert_eq!(
        obj["type"], "roundRect",
        "master preset canonicalised in spec"
    );
}

#[test]
fn wrong_option_value_type_is_reported() {
    let dir = sample_project("badvaltype");
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\nrect_radius = \"1cm\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("rect_radius") && msg.contains("number"),
        "expected a rect_radius type error, got: {msg}"
    );
}

#[test]
fn length_string_margin_is_coerced_to_inches() {
    let dir = sample_project("lenmargin");
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1.5in\"\ny = \"1.5in\"\nw = \"4in\"\nh = \"1in\"\ntext = \"hi\"\nmargin = [\"0.13cm\", \"0.25cm\", \"0.13cm\", \"0.25cm\"]\n",
    );
    let project = gwen::model::Project::load(&dir).unwrap();
    let spec: serde_json::Value =
        serde_json::from_str(&gwen::render::spec_json(&project).unwrap()).unwrap();
    let shapes = spec["sections"][0]["slides"][1]["shapes"]
        .as_array()
        .unwrap();
    let text = shapes.iter().find(|s| s["kind"] == "text").unwrap();
    match &text["margin"] {
        serde_json::Value::Array(items) => {
            assert_eq!(items.len(), 4, "margin is a 4-array");
            for (v, want) in items.iter().zip([0.0512, 0.0984, 0.0512, 0.0984]) {
                assert!(
                    (v.as_f64().unwrap() - want).abs() < 0.001,
                    "margin entry {v} != ~{want} inches"
                );
            }
        }
        other => panic!("margin not coerced to inches: {other}"),
    }
}

#[test]
fn snake_shape_option_is_canonical_in_spec() {
    let dir = sample_project("snakeoptshape");
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"Box\"\nshape = \"round_rect\"\n",
    );
    let project = gwen::model::Project::load(&dir).unwrap();
    let spec: serde_json::Value =
        serde_json::from_str(&gwen::render::spec_json(&project).unwrap()).unwrap();
    let shapes = spec["sections"][0]["slides"][0]["shapes"]
        .as_array()
        .unwrap();
    assert_eq!(
        shapes[0]["shape"], "roundRect",
        "snake `shape` option canonicalised in the spec"
    );
}

#[test]
fn bad_value_reports_source_location() {
    let dir = sample_project("located");
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\ntext = \"hi\"\nfont_size = \"big\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("content.toml"),
        "diagnostic locates the slide file, got: {msg}"
    );
    assert!(
        msg.contains("font_size"),
        "diagnostic names the option: {msg}"
    );
    assert!(
        msg.contains("must be a number"),
        "diagnostic explains the failure: {msg}"
    );
}

#[test]
fn image_w_only_derives_height_from_aspect() {
    let dir = sample_project("imgwonly");
    std::fs::write(dir.join("media").join("pixel.png"), png_bytes(200, 100)).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"image\"\nsrc = \"media/pixel.png\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\n",
    );
    let project = gwen::model::Project::load(&dir).unwrap();
    let spec: serde_json::Value =
        serde_json::from_str(&gwen::render::spec_json(&project).unwrap()).unwrap();
    let shapes = spec["sections"][0]["slides"][1]["shapes"]
        .as_array()
        .unwrap();
    let img = shapes.iter().find(|s| s["kind"] == "image").unwrap();
    assert!((img["w"].as_f64().unwrap() - 2.0).abs() < 1e-9);
    assert!(
        (img["h"].as_f64().unwrap() - 1.0).abs() < 1e-9,
        "h derived from a 2:1 image, got {}",
        img["h"]
    );
}

#[test]
fn image_h_only_derives_width_from_aspect() {
    let dir = sample_project("imghonly");
    std::fs::write(dir.join("media").join("pixel.png"), png_bytes(200, 100)).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"image\"\nsrc = \"media/pixel.png\"\nx = \"1in\"\ny = \"1in\"\nh = \"1in\"\n",
    );
    let project = gwen::model::Project::load(&dir).unwrap();
    let spec: serde_json::Value =
        serde_json::from_str(&gwen::render::spec_json(&project).unwrap()).unwrap();
    let shapes = spec["sections"][0]["slides"][1]["shapes"]
        .as_array()
        .unwrap();
    let img = shapes.iter().find(|s| s["kind"] == "image").unwrap();
    assert!(
        (img["w"].as_f64().unwrap() - 2.0).abs() < 1e-9,
        "w derived from a 2:1 image, got {}",
        img["w"]
    );
    assert!((img["h"].as_f64().unwrap() - 1.0).abs() < 1e-9);
}

#[test]
fn image_needs_w_or_h() {
    let dir = sample_project("imgneither");
    std::fs::write(dir.join("media").join("pixel.png"), png_bytes(200, 100)).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"image\"\nsrc = \"media/pixel.png\"\nx = \"1in\"\ny = \"1in\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("needs `w` or"),
        "expected a missing-dimension error, got: {err:?}"
    );
}

#[test]
fn image_aspect_from_svg_viewbox() {
    let dir = sample_project("imgsvg");
    write(
        &dir.join("media").join("logo.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 200 100\"><rect width=\"200\" height=\"100\"/></svg>\n",
    );
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"image\"\nsrc = \"media/logo.svg\"\nx = \"1in\"\ny = \"1in\"\nw = \"4in\"\n",
    );
    let project = gwen::model::Project::load(&dir).unwrap();
    let spec: serde_json::Value =
        serde_json::from_str(&gwen::render::spec_json(&project).unwrap()).unwrap();
    let shapes = spec["sections"][0]["slides"][1]["shapes"]
        .as_array()
        .unwrap();
    let img = shapes.iter().find(|s| s["kind"] == "image").unwrap();
    assert!(
        (img["h"].as_f64().unwrap() - 2.0).abs() < 1e-9,
        "svg viewBox aspect applied, got {}",
        img["h"]
    );
}

#[test]
fn image_sizing_disables_derivation() {
    let dir = sample_project("imgsizing");
    std::fs::write(dir.join("media").join("pixel.png"), png_bytes(200, 100)).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"image\"\nsrc = \"media/pixel.png\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nsizing = { type = \"cover\" }\n",
    );
    let project = gwen::model::Project::load(&dir).unwrap();
    let spec: serde_json::Value =
        serde_json::from_str(&gwen::render::spec_json(&project).unwrap()).unwrap();
    let shapes = spec["sections"][0]["slides"][1]["shapes"]
        .as_array()
        .unwrap();
    let img = shapes.iter().find(|s| s["kind"] == "image").unwrap();
    assert!(
        img["h"].as_f64().unwrap() == 0.0,
        "sizing present means no derivation (h stays 0), got {}",
        img["h"]
    );
}

#[test]
fn span_renders_per_run_color() {
    let dir = sample_project("span");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = format!("{main}\n[styles.named.red]\ncolor = \"FF0000\"\nfont_size = 24\n");
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"8in\"\nh = \"1in\"\ntext = \"black with <span class=\\\"red\\\">**bold and red**</span>\"\n",
    );
    let project = gwen::model::Project::load(&dir).unwrap();
    let spec: serde_json::Value =
        serde_json::from_str(&gwen::render::spec_json(&project).unwrap()).unwrap();
    let shapes = spec["sections"][0]["slides"][1]["shapes"]
        .as_array()
        .unwrap();
    let text = shapes.iter().find(|s| s["kind"] == "text").unwrap();
    let runs = text["runs"].as_array().unwrap();
    assert_eq!(runs[0]["text"], "black with ");
    assert!(runs[0].get("color").is_none(), "plain run unstyled");
    assert_eq!(runs[1]["text"], "bold and red");
    assert_eq!(runs[1]["bold"], true);
    assert_eq!(runs[1]["color"], "FF0000");
    assert_eq!(runs[1]["fontSize"], 24);
}

/// Master text gets the same markdown + `<span>` run treatment as slide text:
/// `**bold**` and `<span class="red">` render as runs in the master.
#[test]
fn master_rich_text_renders_runs() {
    let dir = sample_project("mrich");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = format!("{main}\n[styles.named.red]\ncolor = \"FF0000\"\n");
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("masters").join("brand.toml"),
        r#"background = { color = "FFFFFF" }

[[shapes]]
type = "text"
x = "1in"
y = "0.2in"
w = "8in"
h = "0.5in"
text = "Master **bold** and <span class=\"red\">red</span>"
"#,
    );
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"4in\"\nw = \"4in\"\nh = \"1in\"\ntext = \"ok\"\n",
    );
    let out = gwen::build(&dir).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let mut found = false;
    for name in zip_names(&bytes) {
        if !name.ends_with(".xml") {
            continue;
        }
        let s = String::from_utf8(zip_member(&bytes, &name).expect("xml part is present")).unwrap();
        if s.contains("Master ") && s.contains("<a:t>bold</a:t>") && s.contains("FF0000") {
            assert!(s.contains("b=\"1\""), "master bold run");
            found = true;
        }
    }
    assert!(found, "master text rendered with bold + red runs");
}

/// An unknown span class is a build error pointing at the text line.
#[test]
fn span_unknown_class_reports_location() {
    let dir = sample_project("spanbad");
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"8in\"\nh = \"1in\"\ntext = \"with <span class=\\\"nope\\\">bad</span>\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("unknown span style `nope`"), "{msg}");
    assert!(msg.contains("content.toml"), "located diagnostic: {msg}");
    assert!(msg.contains("styles.named.nope"), "fix hint: {msg}");
}

#[test]
fn negative_extent_is_rejected() {
    for axis in ["w", "h"] {
        let dir = sample_project(&format!("negextent-{axis}"));
        let other = if axis == "w" { "h" } else { "w" };
        write(
            &dir.join("slides").join("content.toml"),
            &format!(
                "master = \"brand\"\n\n[[shapes]]\ntype = \"rect\"\nx = \"1in\"\ny = \"1in\"\n{axis} = \"-1in\"\n{other} = \"1in\"\n"
            ),
        );
        let err = gwen::build(&dir).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains(&format!("`{axis}` must not be negative")),
            "negative {axis} is a build error: {msg}"
        );
    }
}

#[test]
fn missing_master_is_reported() {
    let dir = sample_project("nomaster");
    write(
        &dir.join("slides").join("content.toml"),
        "[[shapes]]\ntype = \"text\"\ntext = \"hi\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("missing required `master`"),
        "a slide without a master is rejected: {msg}"
    );
}

#[test]
fn core_properties_name_gwen() {
    let dir = sample_project("metadata");
    let bytes = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    let core = String::from_utf8(zip_member(&bytes, "docProps/core.xml").unwrap()).unwrap();
    assert!(core.contains("<dc:creator>Gwen</dc:creator>"), "{core}");
    assert!(
        core.contains("<cp:lastModifiedBy>Gwen</cp:lastModifiedBy>"),
        "{core}"
    );
    assert!(!core.contains("PptxGenJS"), "no pptxgenjs metadata: {core}");
    assert!(
        core.contains("<dcterms:created xsi:type=\"dcterms:W3CDTF\">20"),
        "created timestamp is emitted: {core}"
    );
    let app = String::from_utf8(zip_member(&bytes, "docProps/app.xml").unwrap()).unwrap();
    assert!(app.contains("<Company>Gwen</Company>"), "{app}");
}

#[test]
fn section_guids_use_uppercase_hex() {
    let dir = sample_project("guids");
    let bytes = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    let xml = String::from_utf8(zip_member(&bytes, "ppt/presentation.xml").unwrap()).unwrap();
    let mut found = 0;
    let mut rest = xml.as_str();
    while let Some(at) = rest.find("id=\"{") {
        rest = &rest[at + "id=\"{".len()..];
        let end = rest.find('}').expect("closed guid");
        let guid = &rest[..end];
        assert!(
            !guid.chars().any(|c| c.is_ascii_lowercase()),
            "guid uses uppercase hex: {guid}"
        );
        found += 1;
        rest = &rest[end..];
    }
    assert!(found > 0, "sections carry guids: {xml}");
}

#[test]
fn placeholders_are_locked() {
    let dir = sample_project("phlocks");
    let bytes = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    for part in ["ppt/slides/slide1.xml", "ppt/slideLayouts/slideLayout1.xml"] {
        let xml = String::from_utf8(zip_member(&bytes, part).unwrap()).unwrap();
        assert!(
            xml.contains("<p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>"),
            "placeholder shapes are not grouped: {part}: {xml}"
        );
    }
}

#[test]
fn slide_number_field_shows_a_placeholder_marker() {
    let dir = sample_project("slnumtext");
    let bytes = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    for (part, want) in [
        ("ppt/slides/slide1.xml", "<a:t>‹#›</a:t>"),
        ("ppt/notesSlides/notesSlide1.xml", "<a:t>‹#›</a:t>"),
    ] {
        let xml = String::from_utf8(zip_member(&bytes, part).unwrap()).unwrap();
        assert!(
            xml.contains(want),
            "slide number field text in {part}: {xml}"
        );
    }
}

#[test]
fn gwen_clean_removes_generated_target() {
    let dir = sample_project("clean");
    gwen::build(&dir).unwrap();
    assert!(dir.join("target").is_dir(), "build creates target/");

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_gwen"))
            .arg("clean")
            .arg(dir.as_os_str())
            .output()
            .unwrap()
    };
    let out = run();
    assert!(
        out.status.success(),
        "gwen clean failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!dir.join("target").exists(), "target/ removed");
    assert!(dir.join("main.toml").is_file(), "sources untouched");

    let again = run();
    assert!(again.status.success(), "cleaning twice succeeds");
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("nothing to clean"),
        "second clean is a no-op: {}",
        String::from_utf8_lossy(&again.stderr)
    );
}

#[test]
fn gwen_clean_requires_a_gwen_project() {
    let dir = tmp("clean-safety");
    std::fs::create_dir_all(dir.join("target")).unwrap();
    std::fs::write(dir.join("target").join("keep"), b"keep").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("clean")
        .arg(dir.as_os_str())
        .output()
        .unwrap();
    assert!(!out.status.success(), "non-gwen directories are refused");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not a gwen project"),
        "handler explains the refusal: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.join("target").join("keep").is_file(),
        "the unrelated target/ is untouched"
    );
}

fn run_render(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("render")
        .arg(dir.as_os_str())
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn gwen_render_html_is_self_contained() {
    let dir = sample_project("render-html");
    let out = run_render(&dir, &["--html"]);
    assert!(
        out.status.success(),
        "render --html failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let target = dir.join("target").join("render");
    assert!(target.join("title.png").exists(), "PNGs are still written");
    assert!(target.join("content.png").exists());

    let html = std::fs::read_to_string(target.join("index.html")).unwrap();
    assert!(html.starts_with("<!doctype html>"), "html document");
    assert!(html.contains("<title>deck</title>"), "deck title");
    assert_eq!(
        html.matches("data:image/png;base64,").count(),
        2,
        "both slides are embedded"
    );
    assert!(html.contains("alt=\"title\""), "slide stems label images");
    assert!(html.contains("alt=\"content\""));
    for stem in ["title", "content"] {
        let caption = format!("<figcaption>{stem}</figcaption>");
        let image = format!("alt=\"{stem}\"");
        assert!(html.contains(&caption), "caption for {stem}: {html}");
        assert!(
            html.find(&caption).unwrap() < html.find(&image).unwrap(),
            "caption comes before the {stem} slide"
        );
    }
    assert!(
        html.contains("body.presenting .slide figcaption { display: none; }"),
        "captions are hidden while presenting"
    );
    assert!(
        !html.contains("src=\"title.png\"") && !html.contains("src=\"content.png\""),
        "images are embedded, not linked: {html}"
    );
    assert!(html.contains("requestFullscreen"), "fullscreen support");
    assert!(html.contains("key === 'f'"), "f toggles presentation mode");
    assert!(html.contains("ArrowRight"), "keyboard navigation");
    assert!(html.contains("Escape"), "Esc leaves presentation mode");
    assert!(
        !html.contains("id=\"hud\""),
        "presentation shows only the slide"
    );
}

#[test]
fn gwen_render_html_uses_the_first_format() {
    let dir = sample_project("render-html-format");
    let png_first = run_render(&dir, &["--format", "png,svg", "--html"]);
    assert!(
        png_first.status.success(),
        "png,svg failed: {}",
        String::from_utf8_lossy(&png_first.stderr)
    );
    let target = dir.join("target").join("render");
    let html = std::fs::read_to_string(target.join("index.html")).unwrap();
    assert!(html.contains("data:image/png;base64,"), "first is PNG");
    assert!(
        !html.contains("data:image/svg+xml;base64,"),
        "SVG is not embedded when PNG comes first"
    );
    assert!(target.join("title.png").exists() && target.join("title.svg").exists());

    let svg_first = run_render(&dir, &["--format", "svg,png", "--html"]);
    assert!(
        svg_first.status.success(),
        "svg,png failed: {}",
        String::from_utf8_lossy(&svg_first.stderr)
    );
    let html = std::fs::read_to_string(target.join("index.html")).unwrap();
    assert!(html.contains("data:image/svg+xml;base64,"), "first is SVG");
    assert!(
        !html.contains("data:image/png;base64,"),
        "PNG is not embedded when SVG comes first"
    );
}

#[test]
fn gwen_render_html_respects_page_selection() {
    let dir = sample_project("render-html-pages");
    let out = run_render(&dir, &["--page", "2", "--html"]);
    assert!(
        out.status.success(),
        "render --page failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let target = dir.join("target").join("render");
    let html = std::fs::read_to_string(target.join("index.html")).unwrap();
    assert_eq!(
        html.matches("data:image/png;base64,").count(),
        1,
        "only the selected slide is embedded"
    );
    assert!(html.contains("alt=\"content\""));
    assert!(html.contains("<figcaption>content</figcaption>"));
    assert!(!html.contains("alt=\"title\""));
    assert!(!html.contains("<figcaption>title</figcaption>"));
    assert!(
        !target.join("title.png").exists(),
        "unselected not rendered"
    );
}

#[test]
fn gwen_render_without_html_removes_a_stale_slideshow() {
    let dir = sample_project("render-html-stale");
    assert!(run_render(&dir, &["--html"]).status.success());
    let target = dir.join("target").join("render");
    assert!(target.join("index.html").exists());

    assert!(run_render(&dir, &[]).status.success());
    assert!(
        !target.join("index.html").exists(),
        "a plain render does not leave a stale slideshow"
    );
    assert!(target.join("title.png").exists());
}

/// A one-slide project whose slide TOML is the given table shape.
fn table_project(name: &str, shape: &str) -> PathBuf {
    let dir = tmp(name);
    write(
        &dir.join("main.toml"),
        r##"[presentation]
title = "table"
width = "13.333in"
height = "7.5in"

[theme]
major_font = "Arial"
minor_font = "Arial"

[[sections]]
title = "T"
slides = ["table.toml"]
"##,
    );
    write(
        &dir.join("masters").join("base.toml"),
        "background = { color = \"FFFFFF\" }\n",
    );
    write(
        &dir.join("slides").join("table.toml"),
        &format!("master = \"base\"\n\n{shape}"),
    );
    dir
}

fn slide_xml(bytes: &[u8], part: &str) -> String {
    String::from_utf8(zip_member(bytes, part).unwrap()).unwrap()
}

#[test]
fn table_emits_graphic_frame_cells_and_styles() {
    let dir = table_project(
        "table-frame",
        r##"[[shapes]]
type = "table"
x = "0.5in"
y = "1in"
w = "10in"
col_styles = [{ w = "3in" }, { w = "3in", align = "right" }, { w = "4in" }]
row_styles = [{ h = "0.5in", bold = true, align = "center" }, { h = "0.5in" }]
border = { color = "999999", width = 1 }
fill = { color = "FFFFFF" }
rows = [
  ["Name", "Qty", "Notes"],
  ["Widget", 3, "ok"],
]
"##,
    );
    let bytes = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    let xml = slide_xml(&bytes, "ppt/slides/slide1.xml");
    assert!(xml.contains("<p:graphicFrame>"), "table frame: {xml}");
    assert!(xml.contains("<a:tbl><a:tblPr/>"), "table body: {xml}");
    assert!(
        xml.contains("name=\"Table 0\""),
        "default object name: {xml}"
    );
    assert_eq!(xml.matches("<a:gridCol").count(), 3);
    assert!(
        xml.contains("<a:gridCol w=\"2743200\"/>"),
        "3in column: {xml}"
    );
    assert!(
        xml.contains("<a:gridCol w=\"3657600\"/>"),
        "4in column: {xml}"
    );
    assert_eq!(xml.matches("<a:tr h=\"457200\">").count(), 2);
    for text in ["Name", "Qty", "Notes", "Widget"] {
        assert!(xml.contains(&format!("<a:t>{text}</a:t>")), "{text}: {xml}");
    }
    assert!(xml.contains("b=\"1\""), "header row bold: {xml}");
    assert!(xml.contains("algn=\"ctr\""), "header row centered: {xml}");
    assert!(
        xml.contains("algn=\"r\""),
        "Qty column right aligned: {xml}"
    );
    assert!(
        xml.contains("srgbClr val=\"999999\""),
        "border color: {xml}"
    );
    assert!(
        xml.contains("<a:solidFill><a:srgbClr val=\"FFFFFF\"/></a:solidFill>"),
        "cell fill: {xml}"
    );
    assert!(xml.contains("marL=\"91440\""), "default margins: {xml}");
    assert!(
        xml.contains("<a:ext cx=\"9144000\" cy=\"914400\"/>"),
        "frame size: {xml}"
    );
}

#[test]
fn table_merges_emit_span_and_continuation_cells() {
    let dir = table_project(
        "table-merge",
        r##"[[shapes]]
type = "table"
x = "1in"
y = "1in"
w = "6in"
rows = [
  ["A", "B", "C"],
  [{ text = "Merged", colspan = 2, rowspan = 2 }, "D"],
  ["E"],
]
"##,
    );
    let bytes = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    let xml = slide_xml(&bytes, "ppt/slides/slide1.xml");
    assert!(
        xml.contains("<a:tc gridSpan=\"2\" rowSpan=\"2\">"),
        "origin span: {xml}"
    );
    assert!(
        xml.contains("<a:tc hMerge=\"1\">"),
        "horizontal continuation: {xml}"
    );
    assert!(
        xml.contains("<a:tc vMerge=\"1\">"),
        "vertical continuation: {xml}"
    );
    assert_eq!(xml.matches("<a:t>Merged</a:t>").count(), 1);
    assert!(xml.contains("<a:t>E</a:t>"), "cell after a rowspan: {xml}");
}

#[test]
fn table_defaults_match_pptxgenjs() {
    let dir = table_project(
        "table-defaults",
        r##"[[shapes]]
type = "table"
x = "1in"
y = "1in"
w = "6in"
rows = [["a", "b"], ["c", "d"]]
"##,
    );
    let bytes = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    let xml = slide_xml(&bytes, "ppt/slides/slide1.xml");
    assert_eq!(xml.matches("<a:gridCol w=\"2743200\"/>").count(), 2);
    assert_eq!(
        xml.matches("<a:tr h=\"0\">").count(),
        2,
        "automatic row heights: {xml}"
    );
    assert!(
        xml.contains("<a:ext cx=\"5486400\" cy=\"914400\"/>"),
        "frame height fallback: {xml}"
    );
    assert!(xml.contains("sz=\"1200\""), "default 12pt: {xml}");
    assert!(
        xml.contains("<a:srgbClr val=\"000000\"/>"),
        "default black: {xml}"
    );
    assert_eq!(
        xml.matches("<a:lnL w=\"0\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:noFill/></a:lnL>")
            .count(),
        4,
        "default no borders on every cell: {xml}"
    );
}

#[test]
fn table_named_and_type_styles_apply() {
    let dir = table_project(
        "table-styles",
        r##"[[shapes]]
type = "table"
x = "1in"
y = "1in"
w = "6in"
styles = "loud"
rows = [["a", "b"]]
"##,
    );
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    std::fs::write(
        dir.join("main.toml"),
        format!("{main}\n[styles.table]\nfont_size = 16\n\n[styles.named.loud]\ncolor = \"FF0000\"\nitalic = true\n"),
    )
    .unwrap();
    let bytes = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    let xml = slide_xml(&bytes, "ppt/slides/slide1.xml");
    assert!(xml.contains("sz=\"1600\""), "table type style size: {xml}");
    assert!(xml.contains("srgbClr val=\"FF0000\""), "named color: {xml}");
    assert!(xml.contains("i=\"1\""), "named italic: {xml}");
}

#[test]
fn table_reports_bad_rows_and_options() {
    for (name, shape, want) in [
        (
            "table-empty",
            "[[shapes]]\ntype = \"table\"\nrows = []\n",
            "must not be empty",
        ),
        (
            "table-ragged",
            "[[shapes]]\ntype = \"table\"\nrows = [[\"a\", \"b\"], [\"c\"]]\n",
            "missing cells",
        ),
        (
            "table-overflow",
            "[[shapes]]\ntype = \"table\"\nrows = [[\"a\", \"b\"], [{ text = \"x\", colspan = 3 }]]\n",
            "overflows",
        ),
        (
            "table-rowspan",
            "[[shapes]]\ntype = \"table\"\nrows = [[{ text = \"x\", rowspan = 2 }]]\n",
            "past the last",
        ),
        (
            "table-cols",
            "[[shapes]]\ntype = \"table\"\ncol_styles = [{ w = \"1in\" }]\nrows = [[\"a\", \"b\"]]\n",
            "col_styles",
        ),
        (
            "table-rows",
            "[[shapes]]\ntype = \"table\"\nrow_styles = [{ h = \"1in\" }]\nrows = [[\"a\"], [\"b\"]]\n",
            "row_styles",
        ),
        (
            "table-autopage",
            "[[shapes]]\ntype = \"table\"\nauto_page = true\nrows = [[\"a\"]]\n",
            "not supported",
        ),
        (
            "table-cell-key",
            "[[shapes]]\ntype = \"table\"\nrows = [[{ text = \"a\", fount_size = 12 }]]\n",
            "unknown cell option",
        ),
        (
            "table-border-mix",
            "[[shapes]]\ntype = \"table\"\nborder = { type = \"solid\", top = { width = 1 } }\nrows = [[\"a\"]]\n",
            "cannot mix",
        ),
        (
            "table-width-mismatch",
            "[[shapes]]\ntype = \"table\"\nw = \"10in\"\ncol_styles = [{ w = \"2in\" }, { w = \"2in\" }]\nrows = [[\"a\", \"b\"]]\n",
            "sum to",
        ),
    ] {
        let dir = table_project(name, shape);
        let err = gwen::build(&dir).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains(want), "{name}: wanted `{want}` in: {msg}");
    }
}

#[test]
fn table_renders_fills_borders_and_text_in_previews() {
    let dir = table_project(
        "table-preview",
        r##"[[shapes]]
type = "table"
x = "1in"
y = "1in"
w = "6in"
col_styles = [{ w = "3in" }, { w = "3in" }]
row_styles = [{ h = "0.5in" }, { h = "0.5in" }]
border = { color = "0000FF", width = 1 }
rows = [
  [{ text = "Hello", fill = { color = "FF0000" } }, "World"],
  ["one", "two"],
]
"##,
    );
    let out = run_render(&dir, &["--format", "svg", "--page", "1"]);
    assert!(
        out.status.success(),
        "table render failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let svg = std::fs::read_to_string(dir.join("target/render/table.svg")).unwrap();
    for text in ["Hello", "World", "one", "two"] {
        assert!(svg.contains(text), "{text} painted: {svg}");
    }
    assert!(svg.to_lowercase().contains("#ff0000"), "cell fill: {svg}");
    assert!(svg.to_lowercase().contains("#0000ff"), "cell border: {svg}");
}

#[test]
fn table_allows_rows_fully_covered_by_rowspans() {
    let dir = table_project(
        "table-covered-row",
        r##"[[shapes]]
type = "table"
x = "1in"
y = "1in"
w = "6in"
rows = [
  ["A", "B"],
  [{ text = "C", rowspan = 2 }, { text = "D", rowspan = 2 }],
  [],
]
"##,
    );
    let bytes = std::fs::read(gwen::build(&dir).unwrap()).unwrap();
    let xml = slide_xml(&bytes, "ppt/slides/slide1.xml");
    assert_eq!(xml.matches("<a:tc vMerge=\"1\">").count(), 2);
    assert_eq!(xml.matches("<a:t>C</a:t>").count(), 1);
    assert_eq!(xml.matches("<a:t>D</a:t>").count(), 1);
}

#[test]
fn table_is_rejected_on_masters_and_non_table_shapes() {
    let dir = table_project(
        "table-master",
        "[[shapes]]\ntype = \"text\"\ntext = \"x\"\n",
    );
    write(
        &dir.join("masters").join("base.toml"),
        "background = { color = \"FFFFFF\" }\n\n[[shapes]]\ntype = \"table\"\nrows = [[\"a\"]]\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("only supported on slides"),
        "master table rejected: {err:?}"
    );

    write(
        &dir.join("masters").join("base.toml"),
        "background = { color = \"FFFFFF\" }\n",
    );
    write(
        &dir.join("slides").join("table.toml"),
        "master = \"base\"\n\n[[shapes]]\ntype = \"text\"\ntext = \"x\"\nrows = [[\"a\"]]\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    assert!(
        format!("{err:?}").contains("only valid"),
        "rows on a text shape rejected: {err:?}"
    );
}

#[test]
fn gwen_init_scaffolds_an_existing_directory() {
    let home = tmp("init-empty-home");
    let dir = tmp("init-existing");
    std::fs::write(dir.join("README.md"), "keep me\n").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("init")
        .arg(dir.as_os_str())
        .arg("--no-template")
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("README.md")).unwrap(),
        "keep me\n",
        "unrelated files are kept"
    );
    assert!(dir.join("main.toml").is_file());
    assert!(dir.join("slides").join("title.toml").is_file());
    assert!(dir.join("masters").join("base.toml").is_file());
    assert!(dir.join(".agents/skills/gwen/SKILL.md").is_file());
    assert_eq!(
        std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
        "target/\n"
    );
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        main.contains(&format!("title = \"{name}\"")),
        "title uses the directory name: {main}"
    );
    gwen::build(&dir).unwrap();
}

#[test]
fn gwen_init_defaults_to_the_current_directory() {
    let home = tmp("init-cwd-home");
    let dir = tmp("init-cwd");
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("init")
        .arg("--no-template")
        .env("GWEN_HOME", &home)
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen init . failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(main.contains(&format!("title = \"{name}\"")), "{main}");
}

#[test]
fn gwen_init_creates_a_missing_directory() {
    let home = tmp("init-missing-home");
    let base = tmp("init-missing");
    let dir = base.join("deck");
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("init")
        .arg(dir.as_os_str())
        .arg("--no-template")
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen init missing dir failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("main.toml").is_file());
    gwen::build(&dir).unwrap();
}

#[test]
fn gwen_init_refuses_an_existing_project() {
    let home = tmp("init-twice-home");
    let dir = tmp("init-twice");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_gwen"))
            .arg("init")
            .arg(dir.as_os_str())
            .arg("--no-template")
            .env("GWEN_HOME", &home)
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    let again = run();
    assert!(!again.status.success(), "second init is refused");
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("already a gwen project"),
        "clear refusal: {}",
        String::from_utf8_lossy(&again.stderr)
    );
}

#[test]
fn gwen_init_preserves_existing_scaffold_files() {
    let home = tmp("init-preserve-home");
    let dir = tmp("init-preserve");
    write(
        &dir.join("slides").join("title.toml"),
        "master = \"base\"\n\n[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"4in\"\nh = \"1in\"\ntext = \"custom\"\n",
    );
    write(
        &dir.join("masters").join("base.toml"),
        "background = { color = \"FFFFFF\" }\n",
    );
    write(
        &dir.join(".agents")
            .join("skills")
            .join("gwen")
            .join("SKILL.md"),
        "custom skill\n",
    );
    write(&dir.join(".gitignore"), "node_modules/\n");

    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("init")
        .arg(dir.as_os_str())
        .arg("--no-template")
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let title = std::fs::read_to_string(dir.join("slides").join("title.toml")).unwrap();
    assert!(
        title.contains("text = \"custom\""),
        "slide preserved: {title}"
    );
    assert!(
        dir.join("slides").join("intro.toml").is_file(),
        "missing scaffold added"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join(".agents/skills/gwen/SKILL.md")).unwrap(),
        "custom skill\n",
        "skill preserved"
    );
    let ignore = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
    assert!(
        ignore.contains("node_modules/") && ignore.contains("target/"),
        "gitignore merged: {ignore}"
    );
    gwen::build(&dir).unwrap();
}

#[test]
fn gwen_init_uses_the_template_when_one_is_installed() {
    let home = template_home("init-template-home");
    let dir = tmp("init-template");
    let out = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("init")
        .arg(dir.as_os_str())
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "gwen init template failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.join("masters").join("a.toml").is_file(),
        "template master copied"
    );
    assert!(
        dir.join("slides").join("a.toml").is_file(),
        "template slide copied"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("media").join("keep.txt")).unwrap(),
        "payload\n"
    );
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(main.contains(&format!("title = \"{name}\"")), "{main}");
    gwen::build(&dir).unwrap();
}
