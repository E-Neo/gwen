//! Output-validity harness: reopen every rebuilt deck and check that the
//! package is structurally sound. The checks live in
//! `gwen_pptx::engine::validate` so production builds and this harness share
//! one implementation.

use std::path::{Path, PathBuf};

use gwen_pptx::engine::validate;
use gwen_pptx::opc::Package;

#[path = "decks.rs"]
mod decks;

#[path = "support.rs"]
mod support;

use support::{build_project, new_project, run_fail};

fn fixture(name: &str) -> PathBuf {
    decks::deck(name)
}

/// Structural checks on a rebuilt deck: XML well-formedness, content-type
/// coverage, relationship resolution, root/content-type agreement,
/// theme/master/chart structure and notes wiring.
fn assert_valid(path: &Path) {
    let pkg = Package::open(path).expect("reopen rebuilt deck");
    let violations = validate::validate_package(&pkg);
    assert!(
        violations.is_empty(),
        "invalid package {}: {violations:#?}",
        path.display()
    );
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
/// valid package whose notes wiring is complete: notes master part, master
/// rel on every notes slide and the presentation-level reference.
#[test]
fn notes_deck_rebuilds_valid() {
    let project = new_project(&fixture("notes_placeholder.pptx"), "deck");
    let out = build_project(&project);
    assert_valid(&out);
    let notes = read_zip_entry(&out, "ppt/notesSlides/notesSlide1.xml");
    assert!(notes.contains("Slide Image Placeholder"));
    assert!(
        read_zip_entry(&out, "ppt/notesMasters/notesMaster1.xml").contains("p:notesMaster"),
        "notes master part generated"
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

/// A picture shape referencing a media file that does not exist fails the
/// build with a diagnostic naming the missing file; no deck is written.
#[test]
fn missing_media_file_fails_the_build() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide1.md");
    let md = std::fs::read_to_string(&slide).unwrap();
    let marker = "<!-- shape type=\"picture\" name=\"Ghost\" image=\"ghost.png\" left=\"914400\" top=\"914400\" width=\"3657600\" height=\"2743200\" -->\n![](media/ghost.png)\n";
    std::fs::write(&slide, format!("{md}{marker}")).unwrap();

    let stderr = run_fail(&["build", project.to_str().unwrap()]);
    assert!(
        stderr.contains("src/media/ghost.png"),
        "diagnostic names the missing file: {stderr}"
    );
    assert!(
        !project.join("target").exists() || read_zip_optional(&project).is_none(),
        "no output written for an invalid project"
    );
}

/// A slide whose front matter references an undefined layout path fails the
/// build instead of silently falling back to another layout.
#[test]
fn unknown_layout_path_fails_the_build() {
    let project = new_project(&fixture("two_slides.pptx"), "deck");
    let slide = project.join("src").join("slides").join("slide2.md");
    let md = std::fs::read_to_string(&slide).unwrap();
    std::fs::write(&slide, md.replace("layouts/layout1.md", "layouts/nope.md")).unwrap();

    let stderr = run_fail(&["build", project.to_str().unwrap()]);
    assert!(
        stderr.contains("layouts/nope.md"),
        "diagnostic names the bad layout path: {stderr}"
    );
}

/// The output pptx exists only when validation passed.
fn read_zip_optional(project: &Path) -> Option<()> {
    let name = std::fs::read_dir(project.join("target"))
        .ok()?
        .next()?
        .ok()?;
    let file = std::fs::File::open(project.join("target").join(name.path())).ok()?;
    zip::ZipArchive::new(file).ok()?;
    Some(())
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
