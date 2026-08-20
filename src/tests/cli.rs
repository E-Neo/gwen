use std::path::PathBuf;
use std::process::Command;

#[path = "decks.rs"]
mod decks;

#[path = "support.rs"]
mod support;

use support::{build_project, new_project, project_md, read_zip_bytes, read_zip_entry, tmp};

fn fixture(name: &str) -> PathBuf {
    decks::deck(name)
}

#[test]
fn new_creates_project_structure() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    assert!(project.join("src").join("PRESENTATION.md").exists());
    for sub in ["masters", "layouts", "slides", "media"] {
        assert!(project.join("src").join(sub).exists(), "src/{sub} created");
    }
    let md = project_md(&project);
    assert!(md.contains("slide_width: 9144000"));
    assert!(md.contains("src=\"slides/slide1.md\""));
    assert!(md.contains("src=\"slides/slide2.md\""));
    let slide =
        std::fs::read_to_string(project.join("src").join("slides").join("slide1.md")).unwrap();
    assert!(slide.contains("Alpha"), "slide file mirrors the textbox");
}

#[test]
fn new_errors_if_project_exists() {
    let dir = tmp();
    let project = dir.join("deck");
    std::fs::create_dir_all(&project).unwrap();
    let out = Command::new(support::bin())
        .args([
            "new",
            project.to_str().unwrap(),
            "--pptx",
            fixture("two_slides.pptx").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "creating an existing project must fail"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("already exists"),
        "clear error message"
    );
}

#[test]
fn build_produces_roundtripped_deck() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let out = build_project(&project);
    let md = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(md.contains("Alpha"), "slide text written into the deck");
    let pres = read_zip_entry(&out, "ppt/presentation.xml");
    assert_eq!(pres.matches("<p:sldId ").count(), 2);
}

#[test]
fn build_output_name_comes_from_config() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    build_project(&project);
    assert!(
        project.join("target").join("two_slides.pptx").exists(),
        "default name is the template stem"
    );
}

#[test]
fn build_errors_outside_project() {
    let dir = tmp();
    let out = Command::new(support::bin())
        .args(["build", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not a gwen project"),
        "clear error for a non-project"
    );
}

const PNG_1PX: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\x0dIDATx\xda\x63\xf8\xcf\xc0\xf0\x1f\x00\x05\x00\x01\xff\xff\xff\x14\xbb\x00\x00\x00\x00IEND\xaeB`\x82";

/// Rebuild the template fixture as a zip with an injected picture shape and
/// image part, so media extraction can be exercised end to end.
fn inject_image_into_template(deck: &std::path::Path) {
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
        std::io::Read::read_to_end(&mut entry, &mut data).unwrap();
        zw.start_file(&name, opts).unwrap();
        match name.as_str() {
            "ppt/slides/slide1.xml" => {
                let s = String::from_utf8(data).unwrap();
                let pic = "<p:pic><p:nvPicPr><p:cNvPr id=\"21\" name=\"Pic1\"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"rId9\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"100000\" y=\"100000\"/><a:ext cx=\"500000\" cy=\"400000\"/></a:xfrm><a:prstGeom prst=\"rect\"/></p:spPr></p:pic>";
                std::io::Write::write_all(
                    &mut zw,
                    s.replace("</p:spTree>", &format!("{pic}</p:spTree>"))
                        .as_bytes(),
                )
                .unwrap();
            }
            "ppt/slides/_rels/slide1.xml.rels" => {
                let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/></Relationships>"#;
                std::io::Write::write_all(&mut zw, rels.as_bytes()).unwrap();
            }
            _ => std::io::Write::write_all(&mut zw, &data).unwrap(),
        }
    }
    zw.start_file("ppt/media/image1.png", opts).unwrap();
    std::io::Write::write_all(&mut zw, PNG_1PX).unwrap();
    zw.finish().unwrap();
}

#[test]
fn new_extracts_media_and_build_restores_it() {
    let dir = tmp();
    let deck = dir.join("deck.pptx");
    inject_image_into_template(&deck);

    let project = new_project(&deck, "deck");
    let extracted = project.join("src").join("media").join("image1.png");
    assert!(extracted.exists(), "image extracted into src/media");
    assert_eq!(
        std::fs::read(&extracted).unwrap(),
        PNG_1PX,
        "extracted bytes match the embedded image"
    );

    let out = build_project(&project);
    let restored = read_zip_bytes(&out, "ppt/media/image1.png");
    assert_eq!(&restored, PNG_1PX, "image restored into the deck");
    let ct = read_zip_entry(&out, "[Content_Types].xml");
    assert!(
        ct.contains("Default Extension=\"png\""),
        "png default content type emitted for media"
    );
}

#[test]
fn editing_slide_text_roundtrips() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide1.md");
    let body = std::fs::read_to_string(&slide).unwrap();
    std::fs::write(&slide, body.replace("Alpha", "Changed")).unwrap();

    let out = build_project(&project);
    assert!(read_zip_entry(&out, "ppt/slides/slide1.xml").contains("Changed"));
    let out_md = project_md(&project);
    assert!(out_md.contains("src=\"slides/slide1.md\""), "index intact");
}

#[test]
fn editing_table_cell_text_roundtrips() {
    let project = new_project(&fixture("table_chart.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide1.md");
    let body = std::fs::read_to_string(&slide).unwrap();
    std::fs::write(&slide, body.replace("\n| A |  |", "\n| Zed |  |")).unwrap();

    let out = build_project(&project);
    let xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(xml.contains("Zed"), "table cell text written");
    assert!(!xml.contains(">A<"), "old cell text gone");
}

#[test]
fn editing_chart_data_roundtrips() {
    let project = new_project(&fixture("table_chart.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide1.md");
    let body = std::fs::read_to_string(&slide).unwrap();
    std::fs::write(&slide, body.replace("| S1 | 10 | 20 |", "| S1 | 99 | 20 |")).unwrap();

    let out = build_project(&project);
    let chart = read_zip_entry(&out, "ppt/charts/chart1.xml");
    assert!(
        chart.contains("99"),
        "chart value written into the chart part"
    );
    let ct = read_zip_entry(&out, "[Content_Types].xml");
    assert!(
        ct.contains("ppt/charts/chart1.xml"),
        "chart override content type preserved"
    );
}

#[test]
fn editing_theme_color_roundtrips() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let md = std::fs::read_to_string(project.join("src").join("PRESENTATION.md")).unwrap();
    std::fs::write(
        project.join("src").join("PRESENTATION.md"),
        md.replace("accent1: \"4F81BD\"", "accent1: \"FF0000\""),
    )
    .unwrap();

    let out = build_project(&project);
    let theme = read_zip_entry(&out, "ppt/theme/theme1.xml");
    assert!(theme.contains("FF0000"), "theme color written");
    assert!(theme.contains("C0504D"), "other colors preserved");
}

#[test]
fn editing_core_props_roundtrips() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let md = std::fs::read_to_string(project.join("src").join("PRESENTATION.md")).unwrap();
    std::fs::write(
        project.join("src").join("PRESENTATION.md"),
        md.replace(
            "comments: \"generated using python-pptx\"",
            "comments: \"My Deck\"",
        ),
    )
    .unwrap();

    let out = build_project(&project);
    let core = read_zip_entry(&out, "docProps/core.xml");
    assert!(core.contains("My Deck"), "edited core property written");
}

#[test]
fn rich_text_formatting_roundtrips() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide1.md");
    let body = std::fs::read_to_string(&slide).unwrap();
    std::fs::write(&slide, body.replace("Alpha", "**Alpha**")).unwrap();

    let out = build_project(&project);
    let xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(xml.contains("<a:rPr b=\"1\">"), "bold emphasis written");
}

#[test]
fn adding_a_shape_roundtrips() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide1.md");
    let body = std::fs::read_to_string(&slide).unwrap();
    let new_shape = "<!-- shape type=\"textbox\" name=\"New\" id=\"9\" left=\"100000\" top=\"100000\" width=\"5000000\" height=\"500000\" -->\nHi\n\n";
    std::fs::write(&slide, body.replace("\n\n", &format!("\n\n{new_shape}"))).unwrap();

    let out = build_project(&project);
    let xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(xml.contains(">Hi<"), "new shape text written");
    let out_md = project_md(&project);
    assert!(out_md.contains("src=\"slides/slide1.md\""), "index intact");
}

#[test]
fn deleting_a_shape_roundtrips() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide1.md");
    let body = std::fs::read_to_string(&slide).unwrap();
    let shape_block = "<!-- shape type=\"textbox\" auto-shape=\"rect\" class=\"textbox-1\" id=\"2\" name=\"TextBox 1\" left=\"914400\" top=\"914400\" width=\"3657600\" height=\"914400\" -->\nAlpha";
    let body = body.replace(shape_block, "");
    std::fs::write(&slide, body).unwrap();

    let out = build_project(&project);
    let xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(!xml.contains("Alpha"), "shape removed from the slide");
}

#[test]
fn editing_background_roundtrips() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide1.md");
    let body = std::fs::read_to_string(&slide).unwrap();
    let marker = "<!-- shape type=\"textbox\"";
    let body = body.replace(
        marker,
        "<!-- background fill=\"SOLID:FF00FF\" -->\n\n<!-- shape type=\"textbox\"",
    );
    std::fs::write(&slide, body).unwrap();

    let out = build_project(&project);
    let xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(xml.contains("FF00FF"), "background color written");
    let slide2 = read_zip_entry(&out, "ppt/slides/slide2.xml");
    assert!(!slide2.contains("FF00FF"), "other slide untouched");
}

#[test]
fn editing_placeholder_text_roundtrips() {
    let project = new_project(&fixture("placeholder.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide1.md");
    let body = std::fs::read_to_string(&slide).unwrap();
    let body = body.replace("\n<span></span>", "\nReworked title");
    std::fs::write(&slide, body).unwrap();

    let out = build_project(&project);
    let xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(xml.contains("Reworked title"), "placeholder text written");
}
