//! Structural validation of a built package. The generator authors correct
//! XML, so this is a safety net: it catches the class of bugs that produced
//! invalid decks in the past (dangling relationship ids, root/content-type
//! mismatches, uncovered parts, missing notes wiring).

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::ooxml::content_types;
use crate::ooxml::package::Package;
use crate::ooxml::rels;

#[derive(Debug, Clone, PartialEq)]
pub struct Violation {
    pub part: String,
    pub message: String,
}

impl Violation {
    fn new(part: impl Into<String>, message: impl Into<String>) -> Self {
        Violation {
            part: part.into(),
            message: message.into(),
        }
    }
}

/// Run every check; an empty result means the package is sound.
pub fn validate(pkg: &Package) -> Vec<Violation> {
    let mut out = Vec::new();
    let uris: Vec<String> = pkg.part_uris().cloned().collect();

    check_content_types(&uris, &mut out);
    check_well_formed(pkg, &uris, &mut out);
    check_relationship_targets(pkg, &mut out);
    check_root_elements(pkg, &uris, &mut out);
    check_rid_references(pkg, &uris, &mut out);
    check_presentation(pkg, &mut out);
    check_master(pkg, &mut out);
    check_notes(pkg, &mut out);
    out
}

fn check_content_types(uris: &[String], out: &mut Vec<Violation>) {
    if let Err(err) = content_types::content_types_xml(uris) {
        out.push(Violation::new("[Content_Types].xml", err.to_string()));
    }
}

fn check_well_formed(pkg: &Package, uris: &[String], out: &mut Vec<Violation>) {
    for uri in uris {
        if !uri.ends_with(".xml") {
            continue;
        }
        if let Some(data) = pkg.part(uri)
            && let Err(err) = scan(data)
        {
            out.push(Violation::new(
                uri.clone(),
                format!("not well-formed XML: {err}"),
            ));
        }
    }
}

fn check_relationship_targets(pkg: &Package, out: &mut Vec<Violation>) {
    for source in pkg.rel_sources() {
        for rel in pkg.relationships(source).unwrap_or_default() {
            if rel.external {
                continue;
            }
            let target = rels::resolve_target(source, &rel.target);
            if pkg.part(&target).is_none() {
                out.push(Violation::new(
                    source.clone(),
                    format!("relationship {} points at missing part `{target}`", rel.id),
                ));
            }
        }
    }
}

/// Expected root element per content type.
fn expected_root(content_type: &str) -> Option<&'static str> {
    let roots = [
        ("presentationml.slide+xml", "p:sld"),
        ("presentationml.slideLayout+xml", "p:sldLayout"),
        ("presentationml.slideMaster+xml", "p:sldMaster"),
        ("presentationml.notesSlide+xml", "p:notesSlide"),
        ("presentationml.notesMaster+xml", "p:notesMaster"),
        ("presentationml.presentation.main+xml", "p:presentation"),
        ("theme+xml", "a:theme"),
        ("core-properties+xml", "cp:coreProperties"),
    ];
    roots
        .iter()
        .find(|(suffix, _)| content_type.ends_with(suffix))
        .map(|(_, root)| *root)
}

fn check_root_elements(pkg: &Package, uris: &[String], out: &mut Vec<Violation>) {
    for uri in uris {
        let Some(ct) = content_types::override_for(uri) else {
            continue;
        };
        let Some(root) = expected_root(ct) else {
            continue;
        };
        let Some(data) = pkg.part(uri) else { continue };
        match first_element(data) {
            Ok(name) if name == root => {}
            Ok(name) => out.push(Violation::new(
                uri.clone(),
                format!(
                    "root element `<{name}>` does not match its content type (expected `<{root}>`)"
                ),
            )),
            Err(err) => out.push(Violation::new(
                uri.clone(),
                format!("cannot read root: {err}"),
            )),
        }
    }
}

fn check_rid_references(pkg: &Package, uris: &[String], out: &mut Vec<Violation>) {
    for uri in uris {
        let Some(data) = pkg.part(uri) else { continue };
        let Some(rels) = pkg.relationships(uri) else {
            continue;
        };
        let ids: Vec<&str> = rels.iter().map(|r| r.id.as_str()).collect();
        for (_, _, attrs) in scan(data).unwrap_or_default() {
            for (key, value) in &attrs {
                let is_ref = key == "r:id" || key.ends_with(":embed") || key.ends_with(":link");
                if is_ref && !ids.contains(&value.as_str()) {
                    out.push(Violation::new(
                        uri.clone(),
                        format!("`{key}=\"{value}\"` has no matching relationship"),
                    ));
                }
            }
        }
    }
}

fn check_presentation(pkg: &Package, out: &mut Vec<Violation>) {
    const URI: &str = "ppt/presentation.xml";
    let Some(data) = pkg.part(URI) else {
        out.push(Violation::new(URI, "presentation part missing"));
        return;
    };
    let nodes = scan(data).unwrap_or_default();
    let has = |name: &str| nodes.iter().any(|(_, n, _)| n == name);
    if !has("p:sldMasterIdLst") {
        out.push(Violation::new(URI, "no `p:sldMasterIdLst`"));
    }
    if !has("p:sldIdLst") {
        out.push(Violation::new(URI, "no `p:sldIdLst`"));
    }
    let mut ids = std::collections::HashSet::new();
    for (_, name, attrs) in &nodes {
        if name == "p:sldId" {
            let id = attrs
                .iter()
                .find(|(k, _)| k == "id")
                .and_then(|(_, v)| v.parse::<u32>().ok());
            match id {
                Some(id) if id < 256 => {
                    out.push(Violation::new(URI, format!("slide id {id} is below 256")));
                }
                Some(id) if !ids.insert(id) => {
                    out.push(Violation::new(URI, format!("duplicate slide id {id}")));
                }
                _ => {}
            }
        }
    }
}

fn check_master(pkg: &Package, out: &mut Vec<Violation>) {
    const URI: &str = "ppt/slideMasters/slideMaster1.xml";
    let Some(data) = pkg.part(URI) else {
        out.push(Violation::new(URI, "slide master missing"));
        return;
    };
    let nodes = scan(data).unwrap_or_default();
    if !nodes.iter().any(|(_, n, _)| n == "a:clrMap") {
        out.push(Violation::new(URI, "master has no `a:clrMap`"));
    }
    let layouts = nodes
        .iter()
        .filter(|(_, n, _)| n == "p:sldLayoutId")
        .count();
    if layouts == 0 {
        out.push(Violation::new(URI, "master lists no layouts"));
    }
}

fn check_notes(pkg: &Package, out: &mut Vec<Violation>) {
    let notes_slides: Vec<String> = pkg
        .part_uris()
        .filter(|u| u.starts_with("ppt/notesSlides/notesSlide"))
        .cloned()
        .collect();
    let master_present = pkg.part("ppt/notesMasters/notesMaster1.xml").is_some();
    if notes_slides.is_empty() {
        if master_present {
            out.push(Violation::new(
                "ppt/notesMasters/notesMaster1.xml",
                "notes master exists but no notes slide uses it",
            ));
        }
        return;
    }
    if !master_present {
        out.push(Violation::new(
            "ppt/notesMasters/notesMaster1.xml",
            "notes slides exist but there is no notes master",
        ));
    }
    for uri in &notes_slides {
        let rels = pkg.relationships(uri).unwrap_or_default();
        for (rel_type, what) in [
            (rels::NOTES_MASTER, "the notes master"),
            (rels::SLIDE, "its slide"),
        ] {
            if !rels.iter().any(|r| r.rel_type == rel_type) {
                out.push(Violation::new(
                    uri.clone(),
                    format!("notes slide does not link {what}"),
                ));
            }
        }
    }
    let pres_has = pkg
        .relationships("ppt/presentation.xml")
        .unwrap_or_default()
        .iter()
        .any(|r| r.rel_type == rels::NOTES_MASTER);
    if !pres_has {
        out.push(Violation::new(
            "ppt/presentation.xml",
            "presentation does not reference the notes master",
        ));
    }
}

/// Parse into `(depth, name, attrs)` for every start/empty element.
type Node = (usize, String, Vec<(String, String)>);

fn scan(data: &[u8]) -> Result<Vec<Node>, String> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut nodes = Vec::new();
    let mut depth = 0usize;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                nodes.push((depth, name(e), attrs(e)));
                depth += 1;
            }
            Ok(Event::Empty(ref e)) => nodes.push((depth, name(e), attrs(e))),
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => return Err(err.to_string()),
        }
        buf.clear();
    }
    Ok(nodes)
}

fn first_element(data: &[u8]) -> Result<String, String> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => return Ok(name(e)),
            Ok(Event::Eof) => return Err("no elements".to_string()),
            Ok(_) => {}
            Err(err) => return Err(err.to_string()),
        }
        buf.clear();
    }
}

fn name(e: &quick_xml::events::BytesStart<'_>) -> String {
    e.name().as_ref().to_string()
}

fn attrs(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    e.attributes()
        .flatten()
        .map(|a| (a.key.as_ref().to_string(), a.value.to_string()))
        .collect()
}
