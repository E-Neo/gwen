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
fn text_style_falls_back_for_text_carrying_shape() {
    let dir = sample_project("textfallback");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = format!("{main}\n[styles.rect]\nfont_size = 14\n");
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "master = \"brand\"\n\n[[shapes]]\ntype = \"rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nh = \"1in\"\ntext = \"**Bold** text\"\n",
    );
    let out = gwen::build(&dir).unwrap();
    let xml =
        zip_member(&std::fs::read(&out).unwrap(), "ppt/slides/slide2.xml").expect("slide2 in zip");
    let xml = String::from_utf8(xml).unwrap();
    assert!(
        xml.contains("Arial"),
        "font_face falls back to [styles.text]"
    );
    assert!(
        xml.contains("sz=\"1400\""),
        "font_size from [styles.rect] wins"
    );
    assert!(xml.contains("<a:t>Bold</a:t>"), "text rendered");
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
        ("ppt/slideLayouts/slideLayout2.xml", "master layout"),
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
        xml.contains("<p:ph type=\"sldNum\""),
        "kept as a real slide-number placeholder: {xml}"
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
        a_num.contains("noTextEdit=\"1\""),
        "A is locked because A is locked: {a_num}"
    );
    // Master A's layout stays editable in Slide Master view.
    let a_layout =
        String::from_utf8(zip_member(&bytes, "ppt/slideLayouts/slideLayout2.xml").unwrap())
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
        "[[shapes]]\ntype = \"text\"\ntext = \"from template\"\n",
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
        r##"[[shapes]]
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
        r##"[[shapes]]
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
        "[[shapes]]\ntype = \"round_rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\n",
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
        "[[shapes]]\ntype = \"round_rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\n",
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
        "[[shapes]]\ntype = \"round_rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\n",
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
        "[[shapes]]\ntype = \"rect\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\nrect_radius = \"1cm\"\n",
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
        "[[shapes]]\ntype = \"text\"\nx = \"1.5in\"\ny = \"1.5in\"\nw = \"4in\"\nh = \"1in\"\ntext = \"hi\"\nmargin = [\"0.13cm\", \"0.25cm\", \"0.13cm\", \"0.25cm\"]\n",
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
        "[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"3in\"\nh = \"1in\"\ntext = \"hi\"\nfont_size = \"big\"\n",
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
        "[[shapes]]\ntype = \"image\"\nsrc = \"media/pixel.png\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\n",
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
        "[[shapes]]\ntype = \"image\"\nsrc = \"media/pixel.png\"\nx = \"1in\"\ny = \"1in\"\nh = \"1in\"\n",
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
        "[[shapes]]\ntype = \"image\"\nsrc = \"media/pixel.png\"\nx = \"1in\"\ny = \"1in\"\n",
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
        "[[shapes]]\ntype = \"image\"\nsrc = \"media/logo.svg\"\nx = \"1in\"\ny = \"1in\"\nw = \"4in\"\n",
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
        "[[shapes]]\ntype = \"image\"\nsrc = \"media/pixel.png\"\nx = \"1in\"\ny = \"1in\"\nw = \"2in\"\nsizing = { type = \"cover\" }\n",
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
fn import_roundtrips_a_built_pptx() {
    let dir = sample_project("improundtrip");
    let out = gwen::build(&dir).unwrap();
    let imported = tmp("imported");
    gwen::import_pptx(&out, &imported, None).unwrap();

    assert!(imported.join("main.toml").is_file());
    let main = std::fs::read_to_string(imported.join("main.toml")).unwrap();
    assert!(main.contains("[[sections]]"), "sections present");
    assert!(
        !main.contains("ppt/slides/"),
        "sections reference file names, not package paths: {main}"
    );

    // media files are copied.
    let media: Vec<_> = std::fs::read_dir(imported.join("media"))
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert!(!media.is_empty(), "media copied");

    // Round-trip: the imported project parses, renders a spec and rebuilds.
    let project = gwen::model::Project::load(&imported).unwrap();
    gwen::render::spec_json(&project).unwrap();
    let rebuilt = gwen::build(&imported).unwrap();
    assert!(rebuilt.is_file(), "imported project rebuilds");

    // Markdown emphasis survived the run -> markdown -> run round-trip.
    let all = std::fs::read_dir(imported.join("slides"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        all.contains("**") || all.contains("*"),
        "markdown emphasis preserved in imported slides: {all}"
    );
}

#[test]
fn gwen_new_pptx_cli() {
    let dir = sample_project("newpptx");
    let out = gwen::build(&dir).unwrap();
    let deck = tmp("new-pptx").join("deck");
    let res = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("new")
        .arg(&deck)
        .arg("--pptx")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        res.status.success(),
        "gwen new --pptx failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    // Content comes from the pptx.
    assert!(deck.join("main.toml").is_file());
    assert!(deck.join("slides").is_dir());
    assert!(deck.join("masters").is_dir());
    // `gwen new` semantics: title is the directory name.
    let main = std::fs::read_to_string(deck.join("main.toml")).unwrap();
    let name = deck.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        main.contains(&format!("title = \"{name}\"")),
        "title = dir name: {main}"
    );
    // The project skill is generated like any other `gwen new`.
    assert!(
        deck.join(".agents")
            .join("skills")
            .join("gwen")
            .join("SKILL.md")
            .is_file(),
        "project skill generated"
    );
    // target/ is git-ignored.
    assert_eq!(
        std::fs::read_to_string(deck.join(".gitignore")).unwrap(),
        "target/\n"
    );
    gwen::build(&deck).unwrap();
}

/// `gwen new --pptx` copies the template's skills but not its slides/masters/
/// media - those come from the pptx.
#[test]
fn gwen_new_pptx_with_template_copies_skills_not_content() {
    let home = template_home("home-pptx");
    write(
        &home
            .join("template")
            .join(".agents")
            .join("skills")
            .join("foo")
            .join("SKILL.md"),
        "---\nname: foo\ndescription: A custom skill\n---\n# foo\n",
    );
    let dir = sample_project("pptximp");
    let out = gwen::build(&dir).unwrap();
    let deck = tmp("new-pptx-template").join("deck");
    let res = Command::new(env!("CARGO_BIN_EXE_gwen"))
        .arg("new")
        .arg(&deck)
        .arg("--pptx")
        .arg(&out)
        .env("GWEN_HOME", &home)
        .output()
        .unwrap();
    assert!(
        res.status.success(),
        "gwen new --pptx (with template) failed: {}",
        String::from_utf8_lossy(&res.stderr)
    );
    // Template skills copied.
    assert!(
        deck.join(".agents")
            .join("skills")
            .join("foo")
            .join("SKILL.md")
            .is_file(),
        "template extra skill copied"
    );
    assert!(
        deck.join(".agents")
            .join("skills")
            .join("gwen")
            .join("SKILL.md")
            .is_file(),
        "gwen skill generated"
    );
    // Template content NOT copied - slides come from the imported pptx.
    assert!(
        !deck.join("slides").join("a.toml").is_file(),
        "template slide not copied"
    );
    assert!(
        deck.join("slides").is_dir() && !deck.join("masters").join("a.toml").exists(),
        "content comes from the pptx import"
    );
    gwen::build(&deck).unwrap();
}

/// A layout's picture must be resolved against the layout's own rels, not the
/// master's - otherwise its rId may map to a non-media part and produce a
/// bogus `src = "media/slideLayout1.xml"`.
#[test]
fn import_layout_picture_uses_layout_rels() {
    fn entry(name: &str, data: &[u8]) -> gwen::zip::Entry {
        gwen::zip::Entry {
            name: name.into(),
            data: data.to_vec(),
            dir: false,
        }
    }
    let pres = r##"<?xml version="1.0"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>
<p:sldSz cx="9144000" cy="6858000"/></p:presentation>"##;
    let pres_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"##;
    let master = r##"<?xml version="1.0"?>
<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:cSld name="Master"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Rect 1"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="9144000" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:sp>
</p:spTree></p:cSld></p:sldMaster>"##;
    let master_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"##;
    let layout = r##"<?xml version="1.0"?>
<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:cSld name="Layout"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:pic><p:nvPicPr><p:cNvPr id="2" name="Logo" descr="logo"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>
<p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>
<p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>
</p:spTree></p:cSld></p:sldLayout>"##;
    let layout_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/logo.png"/></Relationships>"##;
    let slide = r##"<?xml version="1.0"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
<p:cSld name="Slide 1"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sld>"##;
    let slide_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;

    let zip_bytes = gwen::zip::write(&[
        entry("ppt/presentation.xml", pres.as_bytes()),
        entry("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
        entry("ppt/slideMasters/slideMaster1.xml", master.as_bytes()),
        entry(
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        entry("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes()),
        entry(
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        entry("ppt/slides/slide1.xml", slide.as_bytes()),
        entry("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
        entry("ppt/media/logo.png", &pixel_png()),
    ]);
    let pptx = tmp("layout-pic");
    std::fs::write(pptx.join("t.pptx"), &zip_bytes).unwrap();
    let imported = tmp("layout-pic-out").join("deck");
    gwen::import_pptx(&pptx.join("t.pptx"), &imported, None).unwrap();

    // The layout's picture resolves to the real media file, never to an .xml part.
    let master_toml =
        std::fs::read_to_string(imported.join("masters").join("slideLayout1.toml")).unwrap();
    assert!(
        master_toml.contains("src = \"media/logo.png\""),
        "layout pic resolved against the layout rels, got: {master_toml}"
    );
    assert!(
        !master_toml.contains(".xml"),
        "no XML part referenced as media: {master_toml}"
    );
    assert!(
        imported.join("media").join("logo.png").is_file(),
        "the real media file was copied"
    );
    gwen::build(&imported).unwrap();
}

/// Import fidelity: a nested `<a:ln><a:solidFill><a:srgbClr .../></a:solidFill></a:ln>`
/// colour, the `<a:latin>` font of a run and a page-number `<a:fld>`, and a
/// slide placeholder inheriting geometry from its layout.
#[test]
fn import_imports_line_color_font_and_placeholder_geometry() {
    fn entry(name: &str, data: &[u8]) -> gwen::zip::Entry {
        gwen::zip::Entry {
            name: name.into(),
            data: data.to_vec(),
            dir: false,
        }
    }
    let pres = r##"<?xml version="1.0"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>
<p:sldSz cx="9144000" cy="6858000"/></p:presentation>"##;
    let pres_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"##;
    let master = r##"<?xml version="1.0"?>
<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:cSld name="Master"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Page Number"/><p:cNvSpPr/><p:nvPr><p:ph type="sldNum"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="5000000" y="6400000"/><a:ext cx="914400" cy="274320"/></a:xfrm></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:fld id="{ABC}" type="slidenum"><a:rPr lang="en-US" sz="974" dirty="0"><a:solidFill><a:srgbClr val="1D1D1B"/></a:solidFill><a:latin typeface="Arial" charset="-122"/></a:rPr><a:t>1</a:t></a:fld></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sldMaster>"##;
    let master_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let layout = r##"<?xml version="1.0"?>
<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:cSld name="Layout"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="3" name="Line"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr>
<p:spPr><a:xfrm><a:off x="1029918" y="1349255"/><a:ext cx="885967" cy="0"/></a:xfrm><a:prstGeom prst="line"><a:avLst/></a:prstGeom><a:ln w="28575"><a:solidFill><a:srgbClr val="C7000B"/></a:solidFill></a:ln></p:spPr></p:cxnSp>
<p:sp><p:nvSpPr><p:cNvPr id="4" name="Title Placeholder"/><p:cNvSpPr/><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="100000" y="200000"/><a:ext cx="500000" cy="300000"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="5" name="Title Text"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="918916" y="630373"/><a:ext cx="1147485" cy="653509"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="zh-CN" sz="3636" dirty="0"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="Microsoft YaHei" charset="-122"/></a:rPr><a:t>标题</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sldLayout>"##;
    let layout_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"##;
    let slide = r##"<?xml version="1.0"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:cSld name="Slide 1"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="标题 2"/><p:cNvSpPr/><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr>
<p:spPr/>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="zh-CN" dirty="0"/><a:t>欢迎</a:t></a:r></a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sld>"##;
    let slide_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;

    let zip_bytes = gwen::zip::write(&[
        entry("ppt/presentation.xml", pres.as_bytes()),
        entry("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
        entry("ppt/slideMasters/slideMaster1.xml", master.as_bytes()),
        entry(
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        entry("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes()),
        entry(
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        entry("ppt/slides/slide1.xml", slide.as_bytes()),
        entry("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
    ]);
    let pptx = tmp("fidelity");
    std::fs::write(pptx.join("t.pptx"), &zip_bytes).unwrap();
    let imported = tmp("fidelity-out").join("deck");
    gwen::import_pptx(&pptx.join("t.pptx"), &imported, None).unwrap();

    let master_toml =
        std::fs::read_to_string(imported.join("masters").join("slideLayout1.toml")).unwrap();
    // The connector's line colour (nested in solidFill) survives.
    assert!(
        master_toml.contains("color = \"C7000B\"") && master_toml.contains("width = 2.25"),
        "connector line colour imported: {master_toml}"
    );
    // The page-number field and the run's <a:latin> font become named run
    // styles in main.toml, referenced by spans in the master text.
    let main_toml = std::fs::read_to_string(imported.join("main.toml")).unwrap();
    assert!(
        main_toml.contains("font_size = 9.74") && main_toml.contains("font_face = \"Arial\""),
        "page-number field font style: {main_toml}"
    );
    assert!(
        main_toml.contains("font_size = 36.36")
            && main_toml.contains("font_face = \"Microsoft YaHei\""),
        "latin font style: {main_toml}"
    );
    assert!(
        master_toml.contains("<span class=\"run"),
        "master text uses spans: {master_toml}"
    );

    // The slide placeholder inherits the layout placeholder's geometry.
    let slide_toml = std::fs::read_to_string(imported.join("slides").join("Slide_1.toml")).unwrap();
    assert!(
        slide_toml.contains("x = 100000")
            && slide_toml.contains("y = 200000")
            && slide_toml.contains("w = 500000")
            && slide_toml.contains("h = 300000"),
        "slide placeholder inherits layout geometry: {slide_toml}"
    );
    assert!(
        slide_toml.contains("欢迎"),
        "slide text present: {slide_toml}"
    );
    gwen::build(&imported).unwrap();
}

/// `<span class="red">` applies a named style's run options to the enclosed
/// markdown runs.
#[test]
fn span_renders_per_run_color() {
    let dir = sample_project("span");
    let main = std::fs::read_to_string(dir.join("main.toml")).unwrap();
    let main = format!("{main}\n[styles.named.red]\ncolor = \"FF0000\"\nfont_size = 24\n");
    std::fs::write(dir.join("main.toml"), main).unwrap();
    write(
        &dir.join("slides").join("content.toml"),
        "[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"8in\"\nh = \"1in\"\ntext = \"black with <span class=\\\"red\\\">**bold and red**</span>\"\n",
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
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut found = false;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        let name = f.name().map(|c| c.into_owned()).unwrap_or_default();
        if !name.ends_with(".xml") {
            continue;
        }
        let mut s = String::new();
        use std::io::Read;
        f.read_to_string(&mut s).unwrap();
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
        "[[shapes]]\ntype = \"text\"\nx = \"1in\"\ny = \"1in\"\nw = \"8in\"\nh = \"1in\"\ntext = \"with <span class=\\\"nope\\\">bad</span>\"\n",
    );
    let err = gwen::build(&dir).unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("unknown span style `nope`"), "{msg}");
    assert!(msg.contains("content.toml"), "located diagnostic: {msg}");
    assert!(msg.contains("styles.named.nope"), "fix hint: {msg}");
}

/// Importing a slide whose runs carry different colours produces
/// `[styles.named.runN]` + `<span class="runN">` text.
#[test]
fn import_emits_span_run_styles() {
    fn entry(name: &str, data: &[u8]) -> gwen::zip::Entry {
        gwen::zip::Entry {
            name: name.into(),
            data: data.to_vec(),
            dir: false,
        }
    }
    let pres = r##"<?xml version="1.0"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>
<p:sldSz cx="9144000" cy="6858000"/></p:presentation>"##;
    let pres_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"##;
    let master = r##"<?xml version="1.0"?>
<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Master"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldMaster>"##;
    let master_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let layout = r##"<?xml version="1.0"?>
<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Layout"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldLayout>"##;
    let layout_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"##;
    let slide = r##"<?xml version="1.0"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Slide 1"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Text"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="100000" y="200000"/><a:ext cx="500000" cy="300000"/></a:xfrm></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p>
<a:r><a:rPr lang="zh-CN" sz="1800" dirty="0"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:latin typeface="Arial" charset="-122"/></a:rPr><a:t>red</a:t></a:r>
<a:r><a:rPr lang="zh-CN" sz="2200" dirty="0"><a:solidFill><a:srgbClr val="0000FF"/></a:solidFill><a:latin typeface="Arial" charset="-122"/></a:rPr><a:t>blue</a:t></a:r>
</a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sld>"##;
    let slide_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let zip_bytes = gwen::zip::write(&[
        entry("ppt/presentation.xml", pres.as_bytes()),
        entry("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
        entry("ppt/slideMasters/slideMaster1.xml", master.as_bytes()),
        entry(
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        entry("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes()),
        entry(
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        entry("ppt/slides/slide1.xml", slide.as_bytes()),
        entry("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
    ]);
    let pptx = tmp("runstyles");
    std::fs::write(pptx.join("t.pptx"), &zip_bytes).unwrap();
    let imported = tmp("runstyles-out").join("deck");
    gwen::import_pptx(&pptx.join("t.pptx"), &imported, None).unwrap();

    let main = std::fs::read_to_string(imported.join("main.toml")).unwrap();
    assert!(main.contains("[styles.named.run1]"), "run style 1: {main}");
    assert!(main.contains("[styles.named.run2]"), "run style 2: {main}");
    assert!(main.contains("color = \"FF0000\""), "run1 colour: {main}");
    assert!(main.contains("color = \"0000FF\""), "run2 colour: {main}");
    let slide_toml = std::fs::read_to_string(imported.join("slides").join("Slide_1.toml")).unwrap();
    assert!(
        slide_toml.contains("<span class=\"run1\">red</span>"),
        "red run wrapped in a span: {slide_toml}"
    );
    assert!(
        slide_toml.contains("<span class=\"run2\">blue</span>"),
        "blue run wrapped in a span: {slide_toml}"
    );
    gwen::build(&imported).unwrap();
}

/// A bold run ending in a space must import as bold markdown whose closing
/// `**` still parses - not as literal asterisks.
#[test]
fn import_bold_run_with_trailing_space_renders_bold() {
    fn entry(name: &str, data: &[u8]) -> gwen::zip::Entry {
        gwen::zip::Entry {
            name: name.into(),
            data: data.to_vec(),
            dir: false,
        }
    }
    let pres = r##"<?xml version="1.0"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>
<p:sldSz cx="9144000" cy="6858000"/></p:presentation>"##;
    let pres_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"##;
    let master = r##"<?xml version="1.0"?>
<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Master"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldMaster>"##;
    let master_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let layout = r##"<?xml version="1.0"?>
<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Layout"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldLayout>"##;
    let layout_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"##;
    let slide = r##"<?xml version="1.0"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Slide 1"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Text"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="100000" y="200000"/><a:ext cx="500000" cy="300000"/></a:xfrm></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p>
<a:r><a:rPr lang="en-US" sz="850" b="1" dirty="0"><a:solidFill><a:srgbClr val="1D1D1B"/></a:solidFill><a:latin typeface="Arial" charset="-122"/></a:rPr><a:t>Copyright©2021 </a:t></a:r>
</a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sld>"##;
    let slide_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let zip_bytes = gwen::zip::write(&[
        entry("ppt/presentation.xml", pres.as_bytes()),
        entry("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
        entry("ppt/slideMasters/slideMaster1.xml", master.as_bytes()),
        entry(
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        entry("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes()),
        entry(
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        entry("ppt/slides/slide1.xml", slide.as_bytes()),
        entry("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
    ]);
    let pptx = tmp("boldspace");
    std::fs::write(pptx.join("t.pptx"), &zip_bytes).unwrap();
    let imported = tmp("boldspace-out").join("deck");
    gwen::import_pptx(&pptx.join("t.pptx"), &imported, None).unwrap();

    // The imported text must be a bold span with the space tucked outside the
    // `**` markers so the build renders bold, never literal asterisks.
    let slide_toml = std::fs::read_to_string(imported.join("slides").join("Slide_1.toml")).unwrap();
    assert!(
        slide_toml.contains("**Copyright©2021** "),
        "trailing space moved outside the markers: {slide_toml}"
    );
    let out = gwen::build(&imported).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut found = false;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        let name = f.name().map(|c| c.into_owned()).unwrap_or_default();
        if !name.ends_with(".xml") {
            continue;
        }
        let mut s = String::new();
        use std::io::Read;
        f.read_to_string(&mut s).unwrap();
        if s.contains("Copyright") {
            let i = s.find("Copyright").unwrap();
            let ctx = &s[i.saturating_sub(300)..(i + 300).min(s.len())];
            assert!(ctx.contains("b=\"1\""), "copyright run is bold: {ctx}");
            assert!(!ctx.contains("**"), "no literal asterisks: {ctx}");
            found = true;
        }
    }
    assert!(found, "copyright text present in output");
}

/// OOXML `baseline="0"` is the normal baseline; it must not become subscript.
#[test]
fn import_baseline_zero_not_subscript() {
    fn entry(name: &str, data: &[u8]) -> gwen::zip::Entry {
        gwen::zip::Entry {
            name: name.into(),
            data: data.to_vec(),
            dir: false,
        }
    }
    let pres = r##"<?xml version="1.0"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>
<p:sldSz cx="9144000" cy="6858000"/></p:presentation>"##;
    let pres_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"##;
    let master = r##"<?xml version="1.0"?>
<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Master"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldMaster>"##;
    let master_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let layout = r##"<?xml version="1.0"?>
<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Layout"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldLayout>"##;
    let layout_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"##;
    let slide = r##"<?xml version="1.0"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Slide 1"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Text"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="100000" y="200000"/><a:ext cx="500000" cy="300000"/></a:xfrm></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p>
<a:r><a:rPr lang="en-US" sz="1000" baseline="0" dirty="0"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:latin typeface="Arial" charset="-122"/></a:rPr><a:t>plain</a:t></a:r>
</a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sld>"##;
    let slide_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let zip_bytes = gwen::zip::write(&[
        entry("ppt/presentation.xml", pres.as_bytes()),
        entry("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
        entry("ppt/slideMasters/slideMaster1.xml", master.as_bytes()),
        entry(
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        entry("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes()),
        entry(
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        entry("ppt/slides/slide1.xml", slide.as_bytes()),
        entry("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
    ]);
    let pptx = tmp("baseline");
    std::fs::write(pptx.join("t.pptx"), &zip_bytes).unwrap();
    let imported = tmp("baseline-out").join("deck");
    gwen::import_pptx(&pptx.join("t.pptx"), &imported, None).unwrap();
    let main = std::fs::read_to_string(imported.join("main.toml")).unwrap();
    assert!(
        !main.contains("subscript"),
        "baseline=0 must not produce subscript: {main}"
    );
    assert!(
        main.contains("color = \"FF0000\""),
        "style has the colour: {main}"
    );
    gwen::build(&imported).unwrap();
}

/// Scheme colour references and theme font tokens resolve against the master's
/// theme: `schemeClr val="tx1"` -> dk1 hex, `latin typeface="+mn-lt"` -> the
/// minor latin font.
#[test]
fn import_resolves_theme_colors_and_fonts() {
    fn entry(name: &str, data: &[u8]) -> gwen::zip::Entry {
        gwen::zip::Entry {
            name: name.into(),
            data: data.to_vec(),
            dir: false,
        }
    }
    let theme = r##"<?xml version="1.0"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<a:themeElements>
<a:clrScheme>
<a:dk1><a:srgbClr val="1D1D1A"/></a:dk1>
<a:lt1><a:srgbClr val="FFFFFF"/></a:lt1>
<a:dk2><a:srgbClr val="000000"/></a:dk2>
<a:lt2><a:srgbClr val="FFFFFF"/></a:lt2>
<a:accent1><a:srgbClr val="C7000A"/></a:accent1>
</a:clrScheme>
<a:fontScheme>
<a:majorFont><a:latin typeface="Arial Black"/></a:majorFont>
<a:minorFont><a:latin typeface="Arial"/></a:minorFont>
</a:fontScheme>
</a:themeElements>
</a:theme>"##;
    let pres = r##"<?xml version="1.0"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>
<p:sldSz cx="9144000" cy="6858000"/></p:presentation>"##;
    let pres_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"##;
    let master = r##"<?xml version="1.0"?>
<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Master"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldMaster>"##;
    let master_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"##;
    let layout = r##"<?xml version="1.0"?>
<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Layout"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldLayout>"##;
    let layout_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"##;
    let slide = r##"<?xml version="1.0"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Slide 1"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Text"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="100000" y="200000"/><a:ext cx="500000" cy="300000"/></a:xfrm></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p>
<a:r><a:rPr lang="en-US" sz="1000" dirty="0"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt" charset="-122"/></a:rPr><a:t>hello</a:t></a:r>
</a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sld>"##;
    let slide_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let zip_bytes = gwen::zip::write(&[
        entry("ppt/presentation.xml", pres.as_bytes()),
        entry("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
        entry("ppt/slideMasters/slideMaster1.xml", master.as_bytes()),
        entry(
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        entry("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes()),
        entry(
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        entry("ppt/slides/slide1.xml", slide.as_bytes()),
        entry("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
        entry("ppt/theme/theme1.xml", theme.as_bytes()),
    ]);
    let pptx = tmp("themere");
    std::fs::write(pptx.join("t.pptx"), &zip_bytes).unwrap();
    let imported = tmp("themere-out").join("deck");
    gwen::import_pptx(&pptx.join("t.pptx"), &imported, None).unwrap();
    let main = std::fs::read_to_string(imported.join("main.toml")).unwrap();
    assert!(
        main.contains("color = \"1D1D1A\""),
        "schemeClr tx1 resolved to dk1 hex: {main}"
    );
    assert!(
        main.contains("font_face = \"Arial\""),
        "+mn-lt resolved to the minor latin font: {main}"
    );
    assert!(
        !main.contains("tx1") && !main.contains("+mn-lt"),
        "no raw theme tokens left: {main}"
    );
    gwen::build(&imported).unwrap();
}

/// A run whose text is whitespace-only (e.g. a spacer run "<a:t> </a:t>")
/// must not panic the importer - it becomes a plain space, no markers.
#[test]
fn import_whitespace_only_run_does_not_panic() {
    fn entry(name: &str, data: &[u8]) -> gwen::zip::Entry {
        gwen::zip::Entry {
            name: name.into(),
            data: data.to_vec(),
            dir: false,
        }
    }
    let pres = r##"<?xml version="1.0"?>
<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst>
<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>
<p:sldSz cx="9144000" cy="6858000"/></p:presentation>"##;
    let pres_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>"##;
    let master = r##"<?xml version="1.0"?>
<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Master"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldMaster>"##;
    let master_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let layout = r##"<?xml version="1.0"?>
<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Layout"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
</p:spTree></p:cSld></p:sldLayout>"##;
    let layout_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"##;
    let slide = r##"<?xml version="1.0"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<p:cSld name="Slide 1"><p:spTree>
<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Text"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
<p:spPr><a:xfrm><a:off x="100000" y="200000"/><a:ext cx="500000" cy="300000"/></a:xfrm></p:spPr>
<p:txBody><a:bodyPr/><a:lstStyle/><a:p>
<a:r><a:rPr lang="en-US" sz="1000" b="1" dirty="0"/><a:t> </a:t></a:r>
</a:p></p:txBody></p:sp>
</p:spTree></p:cSld></p:sld>"##;
    let slide_rels = r##"<?xml version="1.0"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"##;
    let zip_bytes = gwen::zip::write(&[
        entry("ppt/presentation.xml", pres.as_bytes()),
        entry("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
        entry("ppt/slideMasters/slideMaster1.xml", master.as_bytes()),
        entry(
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            master_rels.as_bytes(),
        ),
        entry("ppt/slideLayouts/slideLayout1.xml", layout.as_bytes()),
        entry(
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
            layout_rels.as_bytes(),
        ),
        entry("ppt/slides/slide1.xml", slide.as_bytes()),
        entry("ppt/slides/_rels/slide1.xml.rels", slide_rels.as_bytes()),
    ]);
    let pptx = tmp("wspacerun");
    std::fs::write(pptx.join("t.pptx"), &zip_bytes).unwrap();
    let imported = tmp("wspacerun-out").join("deck");
    gwen::import_pptx(&pptx.join("t.pptx"), &imported, None).unwrap();
    let slide_toml = std::fs::read_to_string(imported.join("slides").join("Slide_1.toml")).unwrap();
    assert!(
        slide_toml.contains(" </span>"),
        "whitespace-only run kept as a plain space: {slide_toml}"
    );
    assert!(
        !slide_toml.contains("**"),
        "no emphasis markers around a bare space: {slide_toml}"
    );
    gwen::build(&imported).unwrap();
}
