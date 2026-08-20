//! Output-validity harness: reopen every rebuilt deck and check that the
//! package is structurally sound — all XML parts parse, all internal
//! relationships resolve, and every part has a content type.

use std::path::{Path, PathBuf};

use gwen_pptx::opc::Package;

#[path = "decks.rs"]
mod decks;

#[path = "support.rs"]
mod support;

use support::{build_project, new_project};

fn fixture(name: &str) -> PathBuf {
    decks::deck(name)
}

/// Structural checks on a rebuilt deck.
fn assert_valid(path: &Path) {
    let pkg = Package::open(path).expect("reopen rebuilt deck");

    // 1. Every XML part is well-formed.
    let xml_uris: Vec<String> = pkg
        .part_uris()
        .filter(|u| u.ends_with(".xml"))
        .cloned()
        .collect();
    for uri in &xml_uris {
        let data = pkg.get_part(uri).expect("part present");
        crate_xml::assert_well_formed(uri, data);
    }

    // 2. Every internal relationship resolves to an existing part.
    for (source, rels) in pkg.rels_uris() {
        for rel in rels.values() {
            if rel.target_mode.as_deref() == Some("External") {
                continue;
            }
            if let Some(target) = pkg.resolve_relationship_target(source, rel) {
                assert!(
                    pkg.part_exists(&target),
                    "{source} -> {target}: relationship target missing"
                );
            }
        }
    }

    // 3. Every part has a content type (default or override).
    let ct = pkg
        .get_part("[Content_Types].xml")
        .expect("[Content_Types].xml present")
        .to_vec();
    let ct = String::from_utf8(ct).expect("content types utf-8");
    for uri in pkg.part_uris() {
        if uri.starts_with('[') && uri.ends_with("].xml") {
            continue;
        }
        let ext = uri.rsplit('.').next().unwrap_or("");
        let covered = ct.contains(&format!("Default Extension=\"{ext}\""))
            || ct.contains(&format!("PartName=\"/{uri}\""));
        assert!(covered, "part {uri} has no content type");
    }

    // 4. Every presentation slide reference resolves to a slide part.
    let pres_rels = pkg
        .get_rels("ppt/presentation.xml")
        .expect("presentation rels");
    let slide_rels = pres_rels.values().filter(|r| {
        r.rel_type == "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide"
    });
    for rel in slide_rels {
        let target = pkg
            .resolve_relationship_target("ppt/presentation.xml", rel)
            .expect("presentation rel resolves");
        assert!(
            target.contains("/slides/slide"),
            "presentation rel points at a slide: {target}"
        );
    }
}

mod crate_xml {
    use quick_xml::Reader;

    pub fn assert_well_formed(uri: &str, data: &[u8]) {
        let mut reader = Reader::from_reader(data);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("{uri} is not well-formed: {e}"),
            }
        }
    }
}

/// Add a slide to a project by editing the slide list and writing a new slide
/// file, then verify the rebuilt package stays structurally sound.
#[test]
fn adding_a_slide_produces_a_valid_package() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let slides = project.join("src").join("slides");

    // Append a slide ref to the index and drop in a fresh slide file.
    let md = std::fs::read_to_string(project.join("src").join("PRESENTATION.md")).unwrap();
    let md = md.replace(
        "<!-- slide src=\"slides/slide2.md\" -->",
        "<!-- slide src=\"slides/slide2.md\" -->\n<!-- slide src=\"slides/slide3.md\" -->",
    );
    std::fs::write(project.join("src").join("PRESENTATION.md"), md).unwrap();

    let slide = "---\nname: \"\"\nlayout: \"layouts/layout1.md\"\n---\n\n<!-- shape type=\"textbox\" name=\"Brand New\" id=\"9\" left=\"100000\" top=\"100000\" width=\"5000000\" height=\"500000\" -->\nFresh\n";
    std::fs::write(slides.join("slide3.md"), slide).unwrap();

    let out = build_project(&project);
    assert_valid(&out);
    assert!(read_zip_entry(&out, "ppt/slides/slide3.xml").contains("Fresh"));
}

/// Delete a slide by removing its reference from the index and removing its
/// mirror file, then verify the rebuilt package stays structurally sound.
#[test]
fn deleting_a_slide_produces_a_valid_package() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let md = std::fs::read_to_string(project.join("src").join("PRESENTATION.md")).unwrap();
    let md = md.replace("<!-- slide src=\"slides/slide2.md\" -->\n", "");
    std::fs::write(project.join("src").join("PRESENTATION.md"), md).unwrap();
    std::fs::remove_file(project.join("src").join("slides").join("slide2.md")).unwrap();

    let out = build_project(&project);
    assert_valid(&out);
    let file = std::fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    assert!(
        zip.by_name("ppt/slides/slide2.xml").is_err(),
        "deleted slide part must be absent from the rebuilt deck"
    );
}

/// A table + chart deck rebuilds into a package whose chart part and table
/// survive structurally.
#[test]
fn table_chart_deck_rebuilds_valid() {
    let project = new_project(&fixture("table_chart.pptx"), "deck");
    let out = build_project(&project);
    assert_valid(&out);
    assert!(read_zip_entry(&out, "ppt/charts/chart1.xml").contains("barChart"));
    assert!(read_zip_entry(&out, "ppt/slides/slide1.xml").contains("a:tbl"));
}

/// The notes slide fixture (a placeholder with no text body) rebuilds into a
/// valid package.
#[test]
fn notes_deck_rebuilds_valid() {
    let project = new_project(&fixture("notes_placeholder.pptx"), "deck");
    let out = build_project(&project);
    assert_valid(&out);
    assert!(
        read_zip_entry(&out, "ppt/notesSlides/notesSlide1.xml").contains("Slide Image Placeholder")
    );
}

fn read_zip_entry(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut entry = zip.by_name(name).unwrap();
    let mut buf = String::new();
    std::io::Read::read_to_string(&mut entry, &mut buf).unwrap();
    buf
}

/// The effects deck rebuilds into a package whose slide carries gradient fills
/// and the full effect list (shadow, glow, soft edge, reflection).
#[test]
fn effects_deck_rebuilds_valid() {
    let project = new_project(&fixture("effects.pptx"), "deck");
    let out = build_project(&project);
    assert_valid(&out);
    let slide = read_zip_entry(&out, "ppt/slides/slide1.xml");
    assert!(slide.contains("a:gradFill"), "linear gradient present");
    assert!(
        slide.contains("<a:path path=\"circle\""),
        "radial gradient present"
    );
    assert!(slide.contains("a:outerShdw"), "outer shadow present");
    assert!(slide.contains("a:innerShdw"), "inner shadow present");
    assert!(slide.contains("a:glow"), "glow present");
    assert!(slide.contains("a:softEdge"), "soft edge present");
    assert!(slide.contains("a:reflection"), "reflection present");
}

/// The effects deck's mirror must express gradients and effects as CSS.
#[test]
fn effects_mirror_uses_css_grammar() {
    let project = new_project(&fixture("effects.pptx"), "deck");
    let slide =
        std::fs::read_to_string(project.join("src").join("slides").join("slide1.md")).unwrap();
    assert!(slide.contains("linear-gradient("), "linear gradient CSS");
    assert!(slide.contains("radial-gradient("), "radial gradient CSS");
    assert!(slide.contains("box-shadow:"), "shadow CSS");
    assert!(slide.contains("--pptx-glow:"), "glow CSS");
    assert!(slide.contains("--pptx-soft-edge:"), "soft edge CSS");
    assert!(slide.contains("--pptx-reflection:"), "reflection CSS");
}
