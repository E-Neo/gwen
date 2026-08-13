use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_pptx-engineer")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn tmp() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "pptx-engineer-it-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_ok(args: &[&str]) -> String {
    let out = Command::new(bin()).args(args).output().expect("run binary");
    assert!(
        out.status.success(),
        "command failed: {args:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn markdown(input: &Path) -> String {
    run_ok(&["markdown", "--input", input.to_str().unwrap()])
}

/// Apply a markdown mirror to `input`, producing a new deck in a temp dir.
fn update(input: &Path, md: &str) -> PathBuf {
    let dir = tmp();
    let md_file = dir.join("deck.md");
    std::fs::write(&md_file, md).unwrap();
    let output = dir.join("out.pptx");
    run_ok(&[
        "update",
        "--input",
        input.to_str().unwrap(),
        "--markdown",
        md_file.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    output
}

fn read_zip_entry(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut entry = zip.by_name(name).unwrap();
    let mut buf = String::new();
    std::io::Read::read_to_string(&mut entry, &mut buf).unwrap();
    buf
}

/// Everything below the `<!-- slide ... -->` marker line for slide `n`
/// (0-based), up to the next marker. Consumes the whole marker line so the
/// closing `-->` never leaks into the block.
fn slide_block(md: &str, n: usize) -> String {
    let mut start = 0;
    for _ in 0..=n {
        let marker = md[start..].find("<!-- slide").expect("slide marker");
        start += marker;
        let line_end = md[start..].find('\n').expect("marker line end");
        start += line_end + 1;
    }
    let next = md[start..]
        .find("<!-- slide")
        .map(|i| start + i)
        .unwrap_or(md.len());
    md[start..next].trim().to_string()
}

#[test]
fn markdown_template_mirror_is_wide() {
    let md = markdown(&fixture("template.pptx"));
    assert!(
        md.contains("slide_width=12192000 slide_height=6858000"),
        "wide deck geometry in the header"
    );
    assert!(
        slide_block(&md, 0).is_empty(),
        "template slide has no shapes"
    );
}

#[test]
fn markdown_template_43_mirror_is_standard() {
    let md = markdown(&fixture("template_43.pptx"));
    assert!(md.contains("slide_width=9144000 slide_height=6858000"));
}

const PNG_1PX: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\x0dIDATx\xda\x63\xf8\xcf\xc0\xf0\x1f\x00\x05\x00\x01\xff\xff\xff\x14\xbb\x00\x00\x00\x00IEND\xaeB`\x82";

/// Rebuild the template fixture as a zip with an injected picture shape and
/// image part, so media extraction can be exercised end to end.
fn inject_image_into_template(deck: &Path) {
    let file = std::fs::File::open(fixture("template.pptx")).unwrap();
    let mut ar = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..ar.len())
        .map(|i| ar.by_index(i).unwrap().name().to_string())
        .collect();
    let out = std::fs::File::create(deck).unwrap();
    let mut zw = zip::ZipWriter::new(out);
    let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    for name in names {
        let mut entry = ar.by_name(&name).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        zw.start_file(&name, opts).unwrap();
        match name.as_str() {
            "ppt/slides/slide1.xml" => {
                let s = String::from_utf8(data).unwrap();
                let pic = "<p:pic><p:nvPicPr><p:cNvPr id=\"21\" name=\"Pic1\"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"rId9\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"100000\" y=\"100000\"/><a:ext cx=\"500000\" cy=\"400000\"/></a:xfrm><a:prstGeom prst=\"rect\"/></p:spPr></p:pic>";
                zw.write_all(
                    s.replace("</p:spTree>", &format!("{pic}</p:spTree>"))
                        .as_bytes(),
                )
                .unwrap();
            }
            "ppt/slides/_rels/slide1.xml.rels" => {
                let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#;
                zw.write_all(rels.as_bytes()).unwrap();
            }
            _ => zw.write_all(&data).unwrap(),
        }
    }
    zw.start_file("ppt/media/image1.png", opts).unwrap();
    zw.write_all(PNG_1PX).unwrap();
    zw.finish().unwrap();
}

#[test]
fn markdown_extracts_media_with_media_flag() {
    let dir = tmp();
    let deck = dir.join("deck.pptx");
    inject_image_into_template(&deck);

    let media_dir = dir.join("media");
    let md = run_ok(&[
        "markdown",
        "--input",
        deck.to_str().unwrap(),
        "--media",
        media_dir.to_str().unwrap(),
    ]);

    let extracted = media_dir.join("image1.png");
    assert!(extracted.exists(), "image extracted into media dir");
    assert_eq!(
        std::fs::read(&extracted).unwrap(),
        PNG_1PX,
        "extracted bytes match the embedded image"
    );
    assert!(
        slide_block(&md, 0).contains("type=picture"),
        "picture shape serialized into the mirror"
    );
}

#[test]
fn update_core_properties_roundtrip() {
    let md = markdown(&fixture("template.pptx"));
    let edited = md.replace(
        "| comments | generated using python-pptx |",
        "| comments | My Deck |",
    );
    let out = update(&fixture("template.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        out_md.contains("| comments | My Deck |"),
        "edited core property round-trips"
    );
}

#[test]
fn update_run_text_roundtrip() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("\nAlpha\n", "\nChanged\n");
    let out = update(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("Changed"),
        "first slide text changed"
    );
    assert!(
        slide_block(&out_md, 1).contains("Beta"),
        "second slide untouched"
    );
}

#[test]
fn update_rich_text_formatting() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("\nAlpha\n", "\n**Alpha**\n");
    let out = update(&fixture("two_slides.pptx"), &edited);
    let slide_xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(
        slide_xml.contains("<a:rPr b=\"1\">"),
        "bold emphasis written to the run properties"
    );
    assert!(
        slide_xml.contains("Alpha"),
        "text preserved through formatting edit"
    );
}

#[test]
fn update_whole_paragraph_replace() {
    // The original pain point: replace a paragraph with two: a bold+size run
    // and a centered one.
    let md = markdown(&fixture("two_slides.pptx"));
    let block = "<!-- shape: type=text_box name=\"TextBox 1\" x=914400 y=914400 w=3657600 h=914400 autoshape=\"rect\" fill={\"type\":\"no_fill\"} -->\n<!-- tf: auto_size=text_to_fit_shape word_wrap=0 -->\nAlpha";
    let two = "<!-- shape: type=text_box name=\"TextBox 1\" x=914400 y=914400 w=3657600 h=914400 autoshape=\"rect\" fill={\"type\":\"no_fill\"} -->\n<!-- tf: auto_size=text_to_fit_shape word_wrap=0 -->\n<span data-size=2000 data-bold=\"true\">Hi</span>\n\n<!-- para: alignment=center -->\nThere";
    let edited = md.replace(block, two);
    let out = update(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    let block = slide_block(&out_md, 0);
    assert!(
        block.contains("<span data-size=2000 data-bold=\"true\">Hi</span>"),
        "first paragraph keeps bold+size"
    );
    assert!(
        block.contains("<!-- para: alignment=center -->\nThere"),
        "second paragraph is centered"
    );
}

#[test]
fn update_delete_paragraph() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("\nAlpha\n", "\n\n");
    let out = update(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    let block = slide_block(&out_md, 0);
    assert!(!block.contains("Alpha"), "paragraph removed from the shape");
    assert!(
        block.contains("type=text_box"),
        "shape survives the paragraph removal"
    );
}

#[test]
fn update_append_paragraph() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace(
        "Alpha\n\n\n<!-- slide -->",
        "Alpha\n\n<!-- para: alignment=center -->\nAppended\n\n<!-- slide -->",
    );
    let out = update(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("Appended"),
        "appended paragraph present in the first slide"
    );
}

#[test]
fn update_background_roundtrip() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replacen(
        "<!-- slide -->",
        "<!-- slide: background=SOLID:FF00FF -->",
        1,
    );
    let out = update(&fixture("two_slides.pptx"), &edited);
    let slide1_xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(
        slide1_xml.contains("<a:srgbClr val=\"FF00FF\"/>"),
        "background color written to the slide"
    );
    let slide2_xml = read_zip_entry(&out, "ppt/slides/slide2.xml");
    assert!(!slide2_xml.contains("FF00FF"), "other slide untouched");
}

#[test]
fn update_table_cell_text() {
    let md = markdown(&fixture("table_chart.pptx"));
    let edited = md.replace("\n| A |  |\n", "\n| Zed |  |\n");
    let out = update(&fixture("table_chart.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("| Zed |  |"),
        "table cell text changed"
    );
}

#[test]
fn update_delete_shape() {
    let md = markdown(&fixture("two_slides.pptx"));
    let block = "<!-- shape: type=text_box name=\"TextBox 1\" x=914400 y=914400 w=3657600 h=914400 autoshape=\"rect\" fill={\"type\":\"no_fill\"} -->\n<!-- tf: auto_size=text_to_fit_shape word_wrap=0 -->\nAlpha\n\n";
    let edited = md.replace(block, "");
    let out = update(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).is_empty(),
        "shape removed from the first slide"
    );
    let slide_xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(
        !slide_xml.contains("Alpha"),
        "shape gone from the slide XML"
    );
}

#[test]
fn update_theme_roundtrip() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md
        .replace("| accent1 | 4F81BD |", "| accent1 | FF0000 |")
        .replace("| major | Calibri |", "| major | Arial |");
    let out = update(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        out_md.contains("| accent1 | FF0000 |"),
        "theme color edited"
    );
    assert!(out_md.contains("| major | Arial |"), "theme font edited");
    assert!(
        out_md.contains("| accent2 | C0504D |"),
        "unedited theme color preserved"
    );
    let theme_xml = read_zip_entry(&out, "ppt/theme/theme1.xml");
    assert!(theme_xml.contains("FF0000"), "color written to theme part");
    assert!(
        theme_xml.contains("typeface=\"Arial\""),
        "font written to theme part"
    );
}

#[test]
fn update_delete_theme_color_by_removing_row_errors() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("| accent1 | 4F81BD |\n", "");
    let dir = tmp();
    let md_file = dir.join("deck.md");
    std::fs::write(&md_file, &edited).unwrap();
    let status = Command::new(bin())
        .args([
            "update",
            "--input",
            fixture("two_slides.pptx").to_str().unwrap(),
            "--markdown",
            md_file.to_str().unwrap(),
            "--output",
            dir.join("out.pptx").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!status.status.success(), "removal must error loudly");
    let stderr = String::from_utf8_lossy(&status.stderr);
    assert!(
        stderr.contains("can only be set"),
        "unsupported removal must be called out, got: {stderr}"
    );
}

#[test]
fn update_delete_text_frame_by_removing_block() {
    let md = markdown(&fixture("two_slides.pptx"));
    let block = "<!-- shape: type=text_box name=\"TextBox 1\" x=914400 y=914400 w=3657600 h=914400 autoshape=\"rect\" fill={\"type\":\"no_fill\"} -->\n<!-- tf: auto_size=text_to_fit_shape word_wrap=0 -->\nAlpha\n\n";
    let shape_only = "<!-- shape: type=text_box name=\"TextBox 1\" x=914400 y=914400 w=3657600 h=914400 autoshape=\"rect\" fill={\"type\":\"no_fill\"} -->\n\n";
    let edited = md.replace(block, shape_only);
    let out = update(&fixture("two_slides.pptx"), &edited);
    let slide_xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(
        !slide_xml.contains("<p:txBody"),
        "removed text frame deletes the shape's txBody"
    );
    let out_md = markdown(&out);
    let block = slide_block(&out_md, 0);
    assert!(
        block.contains("type=auto_shape") && !block.contains("<!-- tf"),
        "shape survives without its text frame (as an auto shape)"
    );
    assert!(
        block.contains("name=\"TextBox 1\""),
        "shape still identified by name"
    );
}

#[test]
fn theme_query_reads_whole_scheme() {
    let md = markdown(&fixture("two_slides.pptx"));
    assert!(md.contains("| accent1 | 4F81BD |"));
    assert!(md.contains("| dk1 |  |"));
}

#[test]
fn master_query_exposes_shapes() {
    let md = markdown(&fixture("two_slides.pptx"));
    assert!(
        md.contains("<!-- shape: type=placeholder name=\"Title Placeholder 1\""),
        "master shapes serialized"
    );
}

#[test]
fn update_master_shape_persists() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace(
        "<!-- shape: type=placeholder name=\"Title Placeholder 1\" x=457200",
        "<!-- shape: type=placeholder name=\"Title Placeholder 1\" x=100000",
    );
    let out = update(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(out_md.contains("x=100000"), "master shape geometry edited");
}

#[test]
fn update_table_row_add_and_remove_roundtrip() {
    let md = markdown(&fixture("table_chart.pptx"));
    let with_row = md.replace("\n| A |  |\n", "\n| A |  |\n| New |  |\n");
    let out = update(&fixture("table_chart.pptx"), &with_row);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("| New |  |"),
        "appended table row present"
    );

    let removed = update(&out, &md);
    let out_md = markdown(&removed);
    assert!(
        !slide_block(&out_md, 0).contains("| New |  |"),
        "row removed when the mirror is reverted"
    );
}

#[test]
fn update_append_shape() {
    let md = markdown(&fixture("two_slides.pptx"));
    let new_shape = "<!-- shape: type=text_box name=\"New\" x=100000 y=100000 w=5000000 h=500000 autoshape=\"rect\" -->\n<!-- tf -->\nHi\n";
    let edited = md.replace(
        "Alpha\n\n\n<!-- slide -->",
        &format!("Alpha\n\n{new_shape}\n\n<!-- slide -->"),
    );
    let out = update(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    let block = slide_block(&out_md, 0);
    assert!(
        block.contains("name=\"New\"") && block.contains("Hi"),
        "appended shape present with its text"
    );
}

#[test]
fn update_notes_slide_create_and_delete() {
    let md = markdown(&fixture("two_slides.pptx"));
    let notes = "<!-- notes -->\n<!-- shape: type=text_box name=\"Notes\" x=100 y=100 w=500 h=300 -->\nNotes!\n";
    let with_notes = md.replace(
        "Alpha\n\n\n<!-- slide -->",
        &format!("Alpha\n\n{notes}\n\n<!-- slide -->"),
    );
    let out = update(&fixture("two_slides.pptx"), &with_notes);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("Notes!"),
        "notes slide created"
    );

    let deleted = update(&out, &md);
    let out_md = markdown(&deleted);
    assert!(
        !slide_block(&out_md, 0).contains("<!-- notes -->"),
        "notes slide removed when the mirror is reverted"
    );
}

#[test]
fn placeholder_inherits_geometry_and_style() {
    let md = markdown(&fixture("placeholder.pptx"));
    let block = slide_block(&md, 0);
    assert!(
        block.contains("type=placeholder name=\"Title 1\" x=685800 y=2130425 w=7772400 h=1470025"),
        "geometry inherited from the slide layout"
    );
    assert!(
        block.contains("font_color=\"SCHEME:tx1\""),
        "text defaults inherited from the layout"
    );
}

#[test]
fn update_default_paragraph_style_roundtrips() {
    let md = markdown(&fixture("placeholder.pptx"));
    let old = "<!-- para: alignment=center font_size=3200 font_name=\"Calibri\" font_bold=true font_color=\"SCHEME:tx1\" -->";
    let new = "<!-- para: alignment=left font_size=2800 font_name=\"Arial\" font_bold=false -->";
    let edited = md.replace(old, new);
    let out = update(&fixture("placeholder.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        out_md.contains(
            "<!-- para: alignment=left font_size=2800 font_name=\"Arial\" font_bold=false -->"
        ),
        "default paragraph style edited"
    );
}

#[test]
fn update_does_not_touch_unmentioned_fields() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("\nAlpha\n", "\nEdited\n");
    let out = update(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    let before = slide_block(&md, 1);
    let after = slide_block(&out_md, 1);
    assert_eq!(
        before, after,
        "untouched slide preserved exactly in the mirror"
    );
    let theme_before = md.split("<!-- master -->").next().unwrap();
    let theme_after = out_md.split("<!-- master -->").next().unwrap();
    assert_eq!(theme_before, theme_after, "theme untouched");
}
