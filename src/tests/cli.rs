use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "decks.rs"]
mod decks;

#[path = "support.rs"]
mod support;

use support::{bin, build, markdown, read_zip_entry, run_ok, slide_block, tmp};

fn fixture(name: &str) -> PathBuf {
    decks::deck(name)
}

#[test]
fn markdown_template_mirror_is_wide() {
    let md = markdown(&fixture("template.pptx"));
    assert!(
        md.contains("slide_width: 12192000"),
        "wide deck geometry in the front matter"
    );
    assert!(
        slide_block(&md, 0).is_empty(),
        "template slide has no shapes"
    );
}

#[test]
fn markdown_template_43_mirror_is_standard() {
    let md = markdown(&fixture("template_43.pptx"));
    assert!(md.contains("slide_width: 9144000"));
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
        md.contains("type=\"picture\""),
        "picture shape serialized into the mirror"
    );
}

#[test]
fn build_core_properties_roundtrip() {
    let md = markdown(&fixture("template.pptx"));
    let edited = md.replace(
        "comments: \"generated using python-pptx\"",
        "comments: \"My Deck\"",
    );
    let out = build(&fixture("template.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        out_md.contains("comments: \"My Deck\""),
        "edited core property round-trips"
    );
}

#[test]
fn build_run_text_roundtrip() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("\nAlpha\n", "\nChanged\n");
    let out = build(&fixture("two_slides.pptx"), &edited);
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
fn build_rich_text_formatting() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("\nAlpha\n", "\n**Alpha**\n");
    let out = build(&fixture("two_slides.pptx"), &edited);
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
fn build_placeholder_text_edit() {
    // A placeholder's paragraph[0] is a normal body paragraph in the mirror;
    // editing it writes through to the placeholder's first run.
    let md = markdown(&fixture("placeholder.pptx"));
    let edited = md.replace(
        "name=\"Title 1\" left=\"685800\" top=\"2130425\" width=\"7772400\" height=\"1470025\" -->\n<span></span>\n\n",
        "name=\"Title 1\" left=\"685800\" top=\"2130425\" width=\"7772400\" height=\"1470025\" -->\nReworked title\n\n",
    );
    let out = build(&fixture("placeholder.pptx"), &edited);
    let slide_xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(
        slide_xml.contains("Reworked title"),
        "placeholder text written to the slide"
    );
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("Reworked title"),
        "edited placeholder text round-trips"
    );
}

#[test]
fn build_whole_paragraph_replace() {
    // Replace a paragraph with two: a bold+size run (via a style block class)
    // and a centered one.
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md
        .replace(
            "\nAlpha\n",
            "\n<span class=\"run-1\">Hi</span>\n\n<!-- paragraph class=\"center-para\" -->\nThere\n",
        )
        .replace(
            "</style>",
            ".run-1 {\n    font-size: 2000;\n    font-weight: bold;\n}\n.center-para {\n    text-align: center;\n}\n</style>",
        );
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    let block = slide_block(&out_md, 0);
    assert!(
        block.contains("<span class=\"run-1\">Hi</span>"),
        "first paragraph keeps bold+size"
    );
    assert!(
        out_md.contains("text-align: center;"),
        "second paragraph is centered"
    );
}

#[test]
fn build_delete_paragraph() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("\nAlpha\n", "\n\n");
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    let block = slide_block(&out_md, 0);
    assert!(!block.contains("Alpha"), "paragraph removed from the shape");
    assert!(
        block.contains("<!-- shape "),
        "shape survives the paragraph removal"
    );
}

#[test]
fn build_append_paragraph() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("Alpha\n\n\n## Slide 2", "Alpha\n\nAppended\n\n## Slide 2");
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("Appended"),
        "appended paragraph present in the first slide"
    );
}

#[test]
fn build_background_roundtrip() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace(
        "## Slide 1\n\n<!-- shape type=\"textbox\"",
        "## Slide 1\n\n<!-- background fill=\"SOLID:FF00FF\" -->\n\n<!-- shape type=\"textbox\"",
    );
    let out = build(&fixture("two_slides.pptx"), &edited);
    let slide1_xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(
        slide1_xml.contains("<a:srgbClr val=\"FF00FF\"/>"),
        "background color written to the slide"
    );
    let slide2_xml = read_zip_entry(&out, "ppt/slides/slide2.xml");
    assert!(!slide2_xml.contains("FF00FF"), "other slide untouched");
}

#[test]
fn build_table_cell_text() {
    let md = markdown(&fixture("table_chart.pptx"));
    let edited = md.replace("\n| A |  |\n", "\n| Zed |  |\n");
    let out = build(&fixture("table_chart.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("| Zed |  |"),
        "table cell text changed"
    );
}

#[test]
fn build_table_cell_text_cleared() {
    let md = markdown(&fixture("table_chart.pptx"));
    let edited = md.replace("\n| A |  |\n", "\n|  |  |\n");
    let out = build(&fixture("table_chart.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("|  |  |"),
        "emptied table cell stays empty"
    );
    let slide1_xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(!slide1_xml.contains(">A<"), "cell text actually removed");
}

#[test]
fn build_delete_shape() {
    let md = markdown(&fixture("two_slides.pptx"));
    let block = "<!-- shape type=\"textbox\" auto-shape=\"rect\" class=\"textbox-1\" name=\"TextBox 1\" left=\"914400\" top=\"914400\" width=\"3657600\" height=\"914400\" -->\nAlpha\n\n\n## Slide 2";
    let edited = md.replace(block, "## Slide 2");
    let out = build(&fixture("two_slides.pptx"), &edited);
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
fn build_theme_roundtrip() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md
        .replace("accent1: \"4F81BD\"", "accent1: \"FF0000\"")
        .replace("major: \"Calibri\"", "major: \"Arial\"");
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(out_md.contains("accent1: \"FF0000\""), "theme color edited");
    assert!(out_md.contains("major: \"Arial\""), "theme font edited");
    assert!(
        out_md.contains("accent2: \"C0504D\""),
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
fn build_delete_theme_color_by_removing_row_errors() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("  accent1: \"4F81BD\"\n", "");
    let dir = tmp();
    let md_file = dir.join("deck.md");
    std::fs::write(&md_file, &edited).unwrap();
    let status = Command::new(bin())
        .args([
            "build",
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
fn build_delete_text_frame_by_removing_block() {
    // A shape's text frame is implied by its body paragraphs; removing them
    // all (leaving only the shape marker) deletes the txBody.
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace(
        "<!-- shape type=\"textbox\" auto-shape=\"rect\" class=\"textbox-1\" name=\"TextBox 1\" left=\"914400\" top=\"914400\" width=\"3657600\" height=\"914400\" -->\nAlpha",
        "<!-- shape type=\"textbox\" auto-shape=\"rect\" class=\"textbox-1\" name=\"TextBox 1\" left=\"914400\" top=\"914400\" width=\"3657600\" height=\"914400\" -->",
    );
    let out = build(&fixture("two_slides.pptx"), &edited);
    let slide_xml = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(
        !slide_xml.contains("<p:txBody"),
        "removed text frame deletes the shape's txBody"
    );
    let out_md = markdown(&out);
    let block = slide_block(&out_md, 0);
    assert!(
        block.contains("<!-- shape "),
        "shape survives without its text frame"
    );
}

#[test]
fn theme_query_reads_whole_scheme() {
    let md = markdown(&fixture("two_slides.pptx"));
    assert!(md.contains("accent1: \"4F81BD\""));
    assert!(md.contains("dk1: \"\""));
}

#[test]
fn master_query_exposes_shapes() {
    let md = markdown(&fixture("two_slides.pptx"));
    assert!(
        md.contains("name=\"Title Placeholder 1\""),
        "master shapes serialized"
    );
}

/// Failed edits surface a human-readable location (with the shape's name) and
/// a next-step suggestion, not a raw path.
#[test]
fn build_failure_diagnostics_point_at_shape() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace(" left=\"914400\"", "");
    let dir = tmp();
    let md_file = dir.join("deck.md");
    std::fs::write(&md_file, &edited).unwrap();
    let out = Command::new(bin())
        .args([
            "build",
            "--input",
            fixture("two_slides.pptx").to_str().unwrap(),
            "--markdown",
            md_file.to_str().unwrap(),
            "--output",
            dir.join("out.pptx").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "removing a shape attribute must fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot be removed from a shape"),
        "clear message, got: {stderr}"
    );
    assert!(
        stderr.contains("Slide 1, shape 1 (TextBox 1)"),
        "human location with shape name, got: {stderr}"
    );
    assert!(
        stderr.contains("Set the attribute to a value"),
        "next-step advice, got: {stderr}"
    );
}

#[test]
fn build_master_shape_persists() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace(
        "<!-- shape type=\"placeholder\" auto-shape=\"rect\" class=\"placeholder-1\" name=\"Title Placeholder 1\" left=\"457200\" top=\"274638\" width=\"8229600\" height=\"1143000\" -->",
        "<!-- shape type=\"placeholder\" auto-shape=\"rect\" class=\"placeholder-1\" name=\"Title Placeholder 1\" left=\"100000\" top=\"274638\" width=\"8229600\" height=\"1143000\" -->",
    );
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        out_md.contains("left=\"100000\""),
        "master shape geometry edited"
    );
}

#[test]
fn build_table_row_add_and_remove_roundtrip() {
    let md = markdown(&fixture("table_chart.pptx"));
    let with_row = md.replace("\n| A |  |\n", "\n| A |  |\n| New |  |\n");
    let out = build(&fixture("table_chart.pptx"), &with_row);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("| New |  |"),
        "appended table row present"
    );

    let removed = build(&out, &md);
    let out_md = markdown(&removed);
    assert!(
        !slide_block(&out_md, 0).contains("| New |  |"),
        "row removed when the mirror is reverted"
    );
}

#[test]
fn build_append_shape() {
    let md = markdown(&fixture("two_slides.pptx"));
    let new_shape = "<!-- shape type=\"textbox\" name=\"New\" left=\"100000\" top=\"100000\" width=\"5000000\" height=\"500000\" -->\nHi\n";
    let edited = md.replace("\n\n## Slide 2", &format!("\n\n{new_shape}\n\n## Slide 2"));
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    let block = slide_block(&out_md, 0);
    assert!(
        block.contains("Hi") && out_md.contains("name=\"New\""),
        "appended shape present with its text"
    );
}

#[test]
fn build_notes_slide_create_and_delete() {
    let md = markdown(&fixture("two_slides.pptx"));
    let notes = "### Notes\n\n<!-- shape type=\"textbox\" name=\"Notes\" left=\"100\" top=\"100\" width=\"500\" height=\"300\" -->\nNotes!\n";
    let with_notes = md.replace("\n\n## Slide 2", &format!("\n\n{notes}\n\n## Slide 2"));
    let out = build(&fixture("two_slides.pptx"), &with_notes);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("Notes!"),
        "notes slide created"
    );

    let deleted = build(&out, &md);
    let out_md = markdown(&deleted);
    assert!(
        !slide_block(&out_md, 0).contains("### Notes"),
        "notes slide removed when the mirror is reverted"
    );
}

#[test]
fn placeholder_inherits_geometry_and_style() {
    let md = markdown(&fixture("placeholder.pptx"));
    let block = slide_block(&md, 0);
    assert!(
        block.contains("type=\"placeholder\"") && block.contains("<!-- shape "),
        "title placeholder serialized as a normal shape"
    );
    assert!(
        md.contains("name=\"Title 1\""),
        "title shape named in the shape marker"
    );
    assert!(
        md.contains("color: SCHEME(tx1)"),
        "text defaults inherited from the layout"
    );
}

#[test]
fn build_default_paragraph_style_roundtrips() {
    // The default paragraph style folds into the shape's styling class; the
    // folded class carries the placeholder's dp (plus its fill and frame
    // properties). The class name is content-addressed, so derive it from the
    // title placeholder's shape marker.
    let md = markdown(&fixture("placeholder.pptx"));
    let marker = md
        .lines()
        .find(|l| l.contains("name=\"Title 1\""))
        .expect("title placeholder marker");
    let sclass = marker
        .split("class=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("styling class");
    let old = format!(
        ".{sclass} {{\n    fill: RGB(C7000B);\n    --pptx-auto-size: shape_to_fit_text;\n    --pptx-vertical-anchor: top;\n    text-align: center;\n    font-size: 3200;\n    font-family: \"Calibri\";\n    font-weight: bold;\n    color: SCHEME(tx1);\n}}"
    );
    let new = format!(
        ".{sclass} {{\n    fill: RGB(C7000B);\n    --pptx-auto-size: shape_to_fit_text;\n    --pptx-vertical-anchor: top;\n    text-align: left;\n    font-size: 2800;\n    font-family: \"Arial\";\n}}"
    );
    let edited = md.replace(&old, &new);
    assert!(edited != md, "styling class found in the mirror");
    let out = build(&fixture("placeholder.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        out_md.contains("text-align: left;\n    font-size: 2800;\n    font-family: \"Arial\";"),
        "default paragraph style edited"
    );
}

#[test]
fn build_does_not_touch_unmentioned_fields() {
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace("\nAlpha\n", "\nEdited\n");
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    let before = slide_block(&md, 1);
    let after = slide_block(&out_md, 1);
    assert_eq!(
        before, after,
        "untouched slide preserved exactly in the mirror"
    );
    let theme_before = md.split("# Master 1").next().unwrap();
    let theme_after = out_md.split("# Master 1").next().unwrap();
    assert_eq!(theme_before, theme_after, "theme untouched");
}

#[test]
fn build_add_slide() {
    let md = markdown(&fixture("two_slides.pptx"));
    let new_slide = "## Slide 2\n\n<!-- shape type=\"textbox\" name=\"Brand New\" left=\"100000\" top=\"100000\" width=\"5000000\" height=\"500000\" -->\nFresh\n\n## Slide 2\n\n";
    let edited = md.replace("\n## Slide 2\n", &format!("\n{new_slide}"));
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 1).contains("Fresh"),
        "inserted slide at index 1"
    );
    assert_eq!(
        slide_block(&out_md, 2),
        slide_block(&md, 1),
        "original second slide shifted, content preserved"
    );
    let pres = read_zip_entry(&out, "ppt/presentation.xml");
    assert_eq!(
        pres.matches("<p:sldId ").count(),
        3,
        "three slide references in the presentation"
    );
    // The inserted slide got its own part, content-type override and rel.
    let ct = read_zip_entry(&out, "[Content_Types].xml");
    assert!(ct.contains("slides/slide3.xml"), "new part registered");
}

#[test]
fn build_delete_slide() {
    // Delete the trailing slide: content-matched slides keep the leading one,
    // so the last slide is unambiguous to remove.
    let md = markdown(&fixture("two_slides.pptx"));
    let second_start = md.find("\n## Slide 2\n").unwrap() + 1;
    let edited = md[..second_start].to_string();
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("Alpha"),
        "leading slide survives"
    );
    let pres = read_zip_entry(&out, "ppt/presentation.xml");
    assert_eq!(
        pres.matches("<p:sldId ").count(),
        1,
        "one slide reference remains"
    );
    // The removed slide's part and content-type override are gone.
    let ct = read_zip_entry(&out, "[Content_Types].xml");
    assert!(
        !ct.contains("slides/slide2.xml"),
        "deleted slide unregistered"
    );
}

#[test]
fn build_delete_slide_keeps_replaced_slide_part_name() {
    // A shape edit on an existing slide is a Replace, so the part keeps its
    // filename instead of being rebuilt under a new name.
    let md = markdown(&fixture("two_slides.pptx"));
    let edited = md.replace(
        "<!-- shape type=\"textbox\" auto-shape=\"rect\" class=\"textbox-1\" name=\"TextBox 1\" left=\"914400\" top=\"914400\" width=\"3657600\" height=\"914400\" -->\nAlpha",
        "<!-- shape type=\"textbox\" auto-shape=\"rect\" class=\"textbox-1\" name=\"TextBox 1\" left=\"914400\" top=\"914400\" width=\"3657600\" height=\"914400\" -->\nAlpha\nBeta",
    );
    let out = build(&fixture("two_slides.pptx"), &edited);
    let slide1 = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(
        slide1.contains("Beta"),
        "slide rebuilt in place under its original part name"
    );
}

#[test]
fn build_move_slide() {
    // Rename the moved slide's shape so its content signature differs from
    // the remaining slide, making the move unambiguous to match.
    let md = markdown(&fixture("two_slides.pptx"));
    let prefix = &md[..md.find("\n## Slide 1\n").unwrap()];
    let alpha_block = slide_block(&md, 0);
    let beta_block = slide_block(&md, 1);
    let renamed_beta = beta_block.replace("name=\"TextBox 1\"", "name=\"Moved Box\"");
    let edited = format!("{prefix}\n## Slide 1\n\n{renamed_beta}\n\n## Slide 2\n\n{alpha_block}\n");
    let out = build(&fixture("two_slides.pptx"), &edited);
    let out_md = markdown(&out);
    assert!(
        slide_block(&out_md, 0).contains("Moved Box"),
        "second slide moved to the front (with its renamed shape)"
    );
}
