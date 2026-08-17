//! Output-validity harness: reopen every rebuilt deck and check that the
//! package is structurally sound — all XML parts parse, all internal
//! relationships resolve, and every part has a content type.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use gwen_pptx::opc::Package;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_gwen")
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
        "gwen-val-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn build(input: &Path, md: &str) -> PathBuf {
    let dir = tmp();
    let md_file = dir.join("deck.md");
    std::fs::write(&md_file, md).unwrap();
    let output = dir.join("out.pptx");
    let out = Command::new(bin())
        .args([
            "build",
            "--input",
            input.to_str().unwrap(),
            "--markdown",
            md_file.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "build failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    output
}

fn markdown(input: &Path) -> String {
    let out = Command::new(bin())
        .args(["markdown", "--input", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "markdown failed");
    String::from_utf8(out.stdout).unwrap()
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

#[test]
fn add_delete_move_slides_produce_valid_packages() {
    let md = markdown(&fixture("two_slides.pptx"));

    // Add a slide in the middle.
    let new_slide = "## Slide 2\n\n<!-- shape type=\"textbox\" name=\"Brand New\" left=\"100000\" top=\"100000\" width=\"5000000\" height=\"500000\" -->\nFresh\n\n## Slide 2\n\n";
    let with_three = build(
        &fixture("two_slides.pptx"),
        &md.replace("\n## Slide 2\n", &format!("\n{new_slide}")),
    );
    assert_valid(&with_three);

    // Delete the trailing slide.
    let second = md.find("\n## Slide 2\n").unwrap() + 1;
    let one = build(&fixture("two_slides.pptx"), &md[..second]);
    assert_valid(&one);

    // Reorder.
    let prefix = &md[..md.find("\n## Slide 1\n").unwrap()];
    let alpha = crate_slide_block(&md, 0);
    let beta = crate_slide_block(&md, 1).replace("name=\"TextBox 1\"", "name=\"Moved Box\"");
    let reordered = format!("{prefix}\n## Slide 1\n\n{beta}\n\n## Slide 2\n\n{alpha}\n");
    let moved = build(&fixture("two_slides.pptx"), &reordered);
    assert_valid(&moved);
}

fn crate_slide_block(md: &str, n: usize) -> String {
    let idxs: Vec<usize> = md.match_indices("\n## ").map(|(i, _)| i + 1).collect();
    let start = idxs[n];
    let line_end = md[start..]
        .find('\n')
        .map(|i| start + i + 1)
        .unwrap_or(md.len());
    let end = idxs.get(n + 1).copied().unwrap_or(md.len());
    md[line_end..end].trim().to_string()
}
