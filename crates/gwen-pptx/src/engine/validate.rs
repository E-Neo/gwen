//! Structural validation of compiled packages. Every rebuild runs through
//! [`validate_package`] before it is written to disk, so an invalid `.pptx`
//! can never reach `target/`.

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::opc::Package;

/// The relationship types whose targets a validator treats specially.
const NOTES_MASTER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
const SLIDE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";

/// One structural problem: the part it concerns and what is wrong with it.
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

/// Validate a package's structure. Returns every violation found; an empty
/// vector means the package is sound.
pub fn validate_package(pkg: &Package) -> Vec<Violation> {
    let mut out = Vec::new();
    validate_well_formed(pkg, &mut out);
    validate_content_types(pkg, &mut out);
    validate_rel_targets(pkg, &mut out);
    validate_roots(pkg, &mut out);
    validate_rids(pkg, &mut out);
    validate_theme(pkg, &mut out);
    validate_presentation(pkg, &mut out);
    validate_masters(pkg, &mut out);
    validate_charts(pkg, &mut out);
    validate_notes(pkg, &mut out);
    out
}

/// One parsed element: `(depth, name, attributes)`.
type Node = (usize, String, Vec<(String, String)>);

/// A parsed XML event stream: every start or empty element with its depth.
/// Depth counts open elements, so children of an element at depth `d` sit at
/// `d + 1`.
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn parse(_part: &str, data: &[u8]) -> Result<Tree, String> {
        let mut reader = Reader::from_reader(data);
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        let mut nodes = Vec::new();
        let mut depth = 0usize;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    nodes.push((
                        depth,
                        e.name().as_ref().to_string(),
                        attrs(e.attributes().flatten()),
                    ));
                    depth += 1;
                }
                Ok(Event::Empty(ref e)) => nodes.push((
                    depth,
                    e.name().as_ref().to_string(),
                    attrs(e.attributes().flatten()),
                )),
                Ok(Event::End(_)) => depth = depth.saturating_sub(1),
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(err) => return Err(format!("not well-formed XML: {err}")),
            }
            buf.clear();
        }
        Ok(Tree { nodes })
    }

    /// The first node named `name`, anywhere in the tree.
    fn find(&self, name: &str) -> Option<&Node> {
        self.nodes.iter().find(|(_, n, _)| n == name)
    }

    /// Every node named `name` whose parent chain puts it directly inside the
    /// first element named `parent`.
    fn children_of(&self, parent: &str) -> Vec<Node> {
        let Some(&(pd, ..)) = self.find(parent) else {
            return Vec::new();
        };
        self.nodes
            .iter()
            .skip_while(|(d, n, _)| !(d == &pd && n == parent))
            .skip(1)
            .take_while(|(d, _, _)| *d > pd)
            .filter(|(d, _, _)| *d == pd + 1)
            .cloned()
            .collect()
    }

    fn attr(node: &Node, key: &str) -> Option<String> {
        node.2
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }
}

fn attrs<'a>(
    iter: impl Iterator<Item = quick_xml::events::attributes::Attribute<'a>>,
) -> Vec<(String, String)> {
    iter.map(|a| (a.key.as_ref().to_string(), a.value.to_string()))
        .collect()
}

/// 1. Every XML part parses.
fn validate_well_formed(pkg: &Package, out: &mut Vec<Violation>) {
    for uri in pkg.part_uris() {
        if !uri.ends_with(".xml") && uri != "[Content_Types].xml" {
            continue;
        }
        let data = pkg.get_part(uri).expect("part present");
        if let Err(err) = Tree::parse(uri, data) {
            out.push(Violation::new(uri.clone(), err));
        }
    }
}

/// 2. Every part is covered by a Default extension or an Override entry.
///
/// Media with extensions PowerPoint cannot render have no content type and
/// would make the whole package unreadable.
fn validate_content_types(pkg: &Package, out: &mut Vec<Violation>) {
    const CT_URI: &str = "[Content_Types].xml";
    let Some(data) = pkg.get_part(CT_URI) else {
        out.push(Violation::new(CT_URI, "[Content_Types].xml missing"));
        return;
    };
    let Ok(tree) = Tree::parse(CT_URI, data) else {
        return; // already reported by the well-formedness check
    };
    let mut defaults: Vec<(String, String)> = Vec::new();
    let mut overrides: Vec<String> = Vec::new();
    for (_, name, at) in &tree.nodes {
        match name.as_str() {
            "Default" => {
                if let (Some(ext), Some(ct)) = (
                    at.iter().find(|(k, _)| k == "Extension"),
                    at.iter().find(|(k, _)| k == "ContentType"),
                ) {
                    defaults.push((ext.1.to_ascii_lowercase(), ct.1.clone()));
                }
            }
            "Override" => {
                if let Some(pn) = at.iter().find(|(k, _)| k == "PartName") {
                    overrides.push(pn.1.trim_start_matches('/').to_string());
                }
            }
            _ => {}
        }
    }
    for uri in pkg.part_uris() {
        if uri == CT_URI {
            continue;
        }
        let ext = uri.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let covered = overrides.iter().any(|o| o == uri) || defaults.iter().any(|(e, _)| *e == ext);
        if !covered {
            out.push(Violation::new(
                uri.clone(),
                format!("no content type covers this part (extension `{ext}` is unknown)"),
            ));
        }
    }
}

/// 3. Every internal relationship resolves to an existing part.
fn validate_rel_targets(pkg: &Package, out: &mut Vec<Violation>) {
    for (source, rels) in pkg.rels_uris() {
        for rel in rels.values() {
            if rel.target_mode.as_deref() == Some("External") {
                continue;
            }
            if let Some(target) = pkg.resolve_relationship_target(source, rel)
                && !pkg.part_exists(&target)
            {
                out.push(Violation::new(
                    source.clone(),
                    format!("relationship {} points at missing part `{target}`", rel.id),
                ));
            }
        }
    }
}

/// Content-type family -> required root element. A mismatch means PowerPoint
/// reads a part as something its XML does not describe (the classic repair
/// prompt).
const ROOT_FOR_TYPE: [(&str, &str); 8] = [
    ("presentationml.slide+xml", "p:sld"),
    ("presentationml.slideLayout+xml", "p:sldLayout"),
    ("presentationml.slideMaster+xml", "p:sldMaster"),
    ("presentationml.notesSlide+xml", "p:notesSlide"),
    ("presentationml.notesMaster+xml", "p:notesMaster"),
    ("presentationml.presentation.main+xml", "p:presentation"),
    ("theme+xml", "a:theme"),
    ("drawingml.chart+xml", "c:chartSpace"),
];

/// 4. Root elements agree with their content types.
fn validate_roots(pkg: &Package, out: &mut Vec<Violation>) {
    const CT_URI: &str = "[Content_Types].xml";
    let ct_data = pkg.get_part(CT_URI).map(|d| d.to_vec()).unwrap_or_default();
    let Ok(ct_tree) = Tree::parse(CT_URI, &ct_data) else {
        return;
    };
    for uri in pkg.part_uris() {
        let Some(data) = pkg.get_part(uri) else {
            continue;
        };
        let Ok(tree) = Tree::parse(uri, data) else {
            continue;
        };
        let Some(root) = tree.nodes.first().map(|(_, n, _)| n.clone()) else {
            continue;
        };
        let expected = ROOT_FOR_TYPE.iter().find_map(|(suffix, tag)| {
            overrides_type(&ct_tree, uri)
                .or_else(|| default_type(&ct_tree, uri))
                .filter(|ct| ct.ends_with(suffix))
                .map(|_| *tag)
        });
        if let Some(tag) = expected
            && root != tag
        {
            out.push(Violation::new(
                uri.clone(),
                format!(
                    "root element `<{root}>` does not match its content type (expected `<{tag}>`)"
                ),
            ));
        }
    }
}

/// The Override content type for `uri`, if any.
fn overrides_type(ct_tree: &Tree, uri: &str) -> Option<String> {
    ct_tree.nodes.iter().find_map(|(_, name, at)| {
        (name == "Override"
            && at
                .iter()
                .any(|(k, v)| k == "PartName" && v.trim_start_matches('/') == uri))
        .then(|| {
            at.iter()
                .find(|(k, _)| k == "ContentType")
                .map(|(_, v)| v.clone())
        })
        .flatten()
    })
}

/// The Default content type covering `uri`'s extension, if any.
fn default_type(ct_tree: &Tree, uri: &str) -> Option<String> {
    let ext = uri.rsplit('.').next()?.to_ascii_lowercase();
    ct_tree.nodes.iter().find_map(|(_, name, at)| {
        (name == "Default"
            && at
                .iter()
                .any(|(k, v)| k == "Extension" && v.eq_ignore_ascii_case(&ext)))
        .then(|| {
            at.iter()
                .find(|(k, _)| k == "ContentType")
                .map(|(_, v)| v.clone())
        })
        .flatten()
    })
}

/// 5. Every `r:id`, `r:embed` and `r:link` reference resolves within the
///    part's own relationships.
fn validate_rids(pkg: &Package, out: &mut Vec<Violation>) {
    for uri in pkg.part_uris() {
        let Some(rels) = pkg.get_rels(uri) else {
            continue;
        };
        let Some(data) = pkg.get_part(uri) else {
            continue;
        };
        let Ok(tree) = Tree::parse(uri, data) else {
            continue;
        };
        for (_, name, at) in &tree.nodes {
            for (key, value) in at {
                let referenced = key == "r:id" || key.ends_with(":embed") || key.ends_with(":link");
                if referenced && !rels.contains_key(value) {
                    out.push(Violation::new(
                        name.clone(),
                        format!("`{key}=\"{value}\"` has no matching relationship"),
                    ));
                }
            }
        }
    }
}

/// 6. The theme carries a complete clrScheme/fontScheme/fmtScheme.
fn validate_theme(pkg: &Package, out: &mut Vec<Violation>) {
    const THEME: &str = "ppt/theme/theme1.xml";
    let Some(data) = pkg.get_part(THEME) else {
        out.push(Violation::new(THEME, "theme part missing"));
        return;
    };
    let Ok(tree) = Tree::parse(THEME, data) else {
        return;
    };

    let slots = tree.children_of("a:clrScheme");
    if slots.len() != 12 {
        out.push(Violation::new(
            THEME,
            format!("clrScheme has {} color slots, expected 12", slots.len()),
        ));
    }
    for (_, name, _) in &slots {
        let kids = tree.children_of(name);
        let ok = kids.iter().any(|(_, k, at)| {
            (k == "a:srgbClr"
                && Tree::attr(&(0, k.clone(), at.clone()), "val")
                    .is_some_and(|v| v.len() == 6 && v.chars().all(|c| c.is_ascii_hexdigit())))
                || k == "a:sysClr"
        });
        if !ok {
            out.push(Violation::new(
                THEME,
                format!("clrScheme slot `{name}` has no valid color"),
            ));
        }
    }

    for family in ["a:majorFont", "a:minorFont"] {
        let faces: Vec<String> = tree
            .children_of(family)
            .iter()
            .map(|(_, n, _)| n.clone())
            .collect();
        for face in ["a:latin", "a:ea", "a:cs"] {
            if !faces.contains(&face.to_string()) {
                out.push(Violation::new(
                    THEME,
                    format!("{family} is missing its `{face}` face"),
                ));
            }
        }
    }

    for list in [
        "a:fillStyleLst",
        "a:lnStyleLst",
        "a:effectStyleLst",
        "a:bgFillStyleLst",
    ] {
        let count = tree.children_of(list).len();
        if count != 3 {
            out.push(Violation::new(
                THEME,
                format!("{list} has {count} entries, expected exactly 3"),
            ));
        }
    }
}

/// 7. The presentation part: geometry, master list, unique slide ids.
fn validate_presentation(pkg: &Package, out: &mut Vec<Violation>) {
    const PRES: &str = "ppt/presentation.xml";
    let Some(data) = pkg.get_part(PRES) else {
        out.push(Violation::new(PRES, "presentation part missing"));
        return;
    };
    let Ok(tree) = Tree::parse(PRES, data) else {
        return;
    };

    let size = tree.find("p:sldSz");
    let ok_size = size
        .map(|s| {
            ["cx", "cy"].iter().all(|k| {
                Tree::attr(s, k)
                    .and_then(|v| v.parse::<i64>().ok())
                    .is_some_and(|v| v > 0)
            })
        })
        .unwrap_or(false);
    if !ok_size {
        out.push(Violation::new(
            PRES,
            "`p:sldSz` must carry a positive cx and cy",
        ));
    }

    if tree.children_of("p:sldMasterIdLst").is_empty() {
        out.push(Violation::new(PRES, "`p:sldMasterIdLst` is empty"));
    }

    let mut seen = std::collections::HashSet::new();
    for entry in tree.children_of("p:sldIdLst") {
        let id = Tree::attr(&entry, "id").and_then(|v| v.parse::<u32>().ok());
        match id {
            Some(id) if id < 256 => {
                out.push(Violation::new(PRES, format!("slide id {id} is below 256")));
            }
            Some(id) if !seen.insert(id) => {
                out.push(Violation::new(PRES, format!("duplicate slide id {id}")));
            }
            _ => {}
        }
    }
}

/// 8. Masters: color map, layout list, package-unique layout ids.
fn validate_masters(pkg: &Package, out: &mut Vec<Violation>) {
    let masters: Vec<String> = pkg
        .part_uris()
        .filter(|u| u.starts_with("ppt/slideMasters/slideMaster"))
        .cloned()
        .collect();
    if masters.is_empty() {
        out.push(Violation::new(
            "ppt/slideMasters",
            "the package has no slide master",
        ));
        return;
    }
    let mut layout_ids = std::collections::HashSet::new();
    for uri in masters {
        let Some(data) = pkg.get_part(&uri) else {
            continue;
        };
        let Ok(tree) = Tree::parse(&uri, data) else {
            continue;
        };
        if tree.find("a:clrMap").is_none() {
            out.push(Violation::new(uri.clone(), "master has no `a:clrMap`"));
        }
        let layouts = tree.children_of("p:sldLayoutIdLst");
        if layouts.is_empty() {
            out.push(Violation::new(
                uri.clone(),
                "master lists no layouts (`p:sldLayoutIdLst` empty)",
            ));
        }
        for layout in layouts {
            match Tree::attr(&layout, "id").and_then(|v| v.parse::<u32>().ok()) {
                Some(id) if id < 2147483648 => {
                    out.push(Violation::new(
                        uri.clone(),
                        format!("layout id {id} is below 2147483648"),
                    ));
                }
                Some(id) if !layout_ids.insert(id) => {
                    out.push(Violation::new(
                        uri.clone(),
                        format!("duplicate layout id {id}"),
                    ));
                }
                _ => {}
            }
        }
    }
}

/// 9. Chart parts carry their required plot structure.
fn validate_charts(pkg: &Package, out: &mut Vec<Violation>) {
    for uri in pkg.part_uris() {
        if !uri.starts_with("ppt/charts/chart") {
            continue;
        }
        let Some(data) = pkg.get_part(uri) else {
            continue;
        };
        let Ok(tree) = Tree::parse(uri, data) else {
            continue;
        };
        let plot_kinds: Vec<String> = tree
            .children_of("c:plotArea")
            .iter()
            .map(|(_, n, _)| n.clone())
            .collect();
        let kind = match plot_kinds
            .iter()
            .find(|n| n.starts_with("c:") && n != &"c:layout")
        {
            Some(k) => k.clone(),
            None => {
                out.push(Violation::new(
                    uri.clone(),
                    "plotArea has no chart type element",
                ));
                continue;
            }
        };
        if kind == "c:barChart" {
            let ax_ids: Vec<String> = tree
                .children_of("c:barChart")
                .iter()
                .filter(|(_, n, _)| n == "c:axId")
                .filter_map(|n| Tree::attr(n, "val"))
                .collect();
            if ax_ids.len() != 2 {
                out.push(Violation::new(
                    uri.clone(),
                    "barChart needs exactly two axId references",
                ));
            }
            for axis in ["c:catAx", "c:valAx"] {
                if tree
                    .children_of("c:plotArea")
                    .iter()
                    .all(|(_, n, _)| n != axis)
                {
                    out.push(Violation::new(
                        uri.clone(),
                        format!("barChart is missing `{axis}`"),
                    ));
                }
            }
        }
    }
}

/// 10. Notes wiring: slides' notes link a master and their slide; the
///     notesMasterIdLst matches reality.
fn validate_notes(pkg: &Package, out: &mut Vec<Violation>) {
    let notes_slides: Vec<String> = pkg
        .part_uris()
        .filter(|u| u.starts_with("ppt/notesSlides/notesSlide"))
        .cloned()
        .collect();
    let master_present = pkg.part_exists("ppt/notesMasters/notesMaster1.xml");
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
        let Some(rels) = pkg.get_rels(uri) else {
            out.push(Violation::new(
                uri.clone(),
                "notes slide has no relationships",
            ));
            continue;
        };
        for (ty, what) in [
            (NOTES_MASTER_REL, "the notes master"),
            (SLIDE_REL, "its slide"),
        ] {
            if !rels.values().any(|r| r.rel_type == ty) {
                out.push(Violation::new(
                    uri.clone(),
                    format!("notes slide does not link {what}"),
                ));
            }
        }
    }
    let pres_has_master_ref = pkg
        .get_rels("ppt/presentation.xml")
        .map(|rels| rels.values().any(|r| r.rel_type == NOTES_MASTER_REL))
        .unwrap_or(false);
    if !pres_has_master_ref {
        out.push(Violation::new(
            "ppt/presentation.xml",
            "presentation does not reference the notes master",
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::opc::Relationship;

    const SLIDE_CT: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
    const RELS_TYPE: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

    fn pkg_with(parts: &[(&str, &str)]) -> Package {
        let mut pkg = Package::empty();
        for (uri, data) in parts {
            pkg.set_part(uri, data.as_bytes().to_vec());
        }
        pkg
    }

    fn rel(id: &str, target: &str) -> Relationship {
        Relationship {
            id: id.to_string(),
            target: target.to_string(),
            target_mode: None,
            rel_type: RELS_TYPE.to_string(),
        }
    }

    #[test]
    fn unknown_media_extension_is_reported() {
        let mut pkg = pkg_with(&[
            (
                "[Content_Types].xml",
                "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>",
            ),
            ("ppt/media/photo.webp", ""),
        ]);
        // The generator would refuse first; validate catches hand-built ones.
        let v = validate_package(&pkg);
        assert!(
            v.iter()
                .any(|x| x.message.contains("webp") && x.part == "ppt/media/photo.webp"),
            "{v:#?}"
        );
        pkg.remove_part("ppt/media/photo.webp");
        assert!(
            validate_package(&pkg)
                .iter()
                .all(|x| x.part != "ppt/media/photo.webp")
        );
    }

    #[test]
    fn dangling_rid_is_reported() {
        let mut pkg = pkg_with(&[
            (
                "[Content_Types].xml",
                "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>",
            ),
            (
                "ppt/slides/slide1.xml",
                "<p:sld xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><c:chart r:id=\"rId9\"/></p:sld>",
            ),
        ]);
        pkg.add_relationship("ppt/slides/slide1.xml", rel("rId2", "../x.png"));
        let v = validate_package(&pkg);
        assert!(v.iter().any(|x| x.message.contains("rId9")), "{v:#?}");
    }

    #[test]
    fn root_element_mismatch_is_reported() {
        let pkg = pkg_with(&[
            (
                "[Content_Types].xml",
                &format!(
                    "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/ppt/slides/slide1.xml\" ContentType=\"{SLIDE_CT}\"/></Types>"
                ),
            ),
            ("ppt/slides/slide1.xml", "<p:notesSlide/>"),
        ]);
        let v = validate_package(&pkg);
        assert!(
            v.iter()
                .any(|x| x.message.contains("does not match its content type")),
            "{v:#?}"
        );
    }

    #[test]
    fn missing_theme_is_reported() {
        let pkg = pkg_with(&[("[Content_Types].xml", "<Types/>")]);
        let v = validate_package(&pkg);
        assert!(v.iter().any(|x| x.message.contains("theme part missing")));
    }
}
