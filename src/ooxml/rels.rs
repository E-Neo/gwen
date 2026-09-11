//! OPC relationships: relationship types, relative-target resolution, and the
//! `.rels` part serializer.

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};

pub const OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
pub const SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
pub const SLIDE_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster";
pub const SLIDE_LAYOUT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
pub const THEME: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
pub const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
pub const NOTES_SLIDE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";
pub const NOTES_MASTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster";
pub const PRES_PROPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/presProps";
pub const VIEW_PROPS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/viewProps";
pub const TABLE_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/tableStyles";
pub const EXTENDED_PROPERTIES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
pub const CORE_PROPERTIES: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";

/// One relationship from a source part (empty string = package root).
#[derive(Debug, Clone, PartialEq)]
pub struct Relationship {
    pub id: String,
    pub rel_type: String,
    pub target: String,
    pub external: bool,
}

/// The `.rels` part name for a source part URI (`""` is the package root).
pub fn rels_part_name(source_uri: &str) -> String {
    if source_uri.is_empty() {
        return "_rels/.rels".to_string();
    }
    let (dir, file) = match source_uri.rsplit_once('/') {
        Some((dir, file)) => (dir, file),
        None => ("", source_uri),
    };
    if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    }
}

/// The relationship target path for `target_uri` as seen from `source_uri`,
/// i.e. a `../`-style relative path. Both are package URIs without a leading
/// slash. This is the single place targets are computed, so the v1 bug class
/// (absolute targets that resolve to `ppt/ppt/...`) cannot recur.
pub fn relative_target(source_uri: &str, target_uri: &str) -> String {
    let source_dir: Vec<&str> = source_uri
        .rsplit_once('/')
        .map(|(dir, _)| dir.split('/').collect())
        .unwrap_or_default();
    let target: Vec<&str> = target_uri.split('/').collect();
    let common = source_dir
        .iter()
        .zip(target.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out: Vec<&str> = vec![".."; source_dir.len() - common];
    out.extend_from_slice(&target[common..]);
    if out.is_empty() {
        ".".to_string()
    } else {
        out.join("/")
    }
}

/// Resolve a relationship target against its source part into a package URI.
/// The inverse of [`relative_target`].
pub fn resolve_target(source_uri: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut segments: Vec<&str> = source_uri
        .rsplit_once('/')
        .map(|(dir, _)| dir.split('/').collect())
        .unwrap_or_default();
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// Serialize a relationship list into a `.rels` part. One `<Relationship>`
/// element per line: PowerPoint and some validators (e.g. ppt-rs's repair
/// utility) read `.rels` line by line, so a single-line document would only
/// ever surface its first relationship.
pub fn rels_xml(rels: &[Relationship]) -> Vec<u8> {
    let mut writer = Writer::new(Vec::new());
    writer
        .write_event(Event::Decl(BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            Some("yes"),
        )))
        .ok();
    writer.write_event(Event::Text(BytesText::new("\n"))).ok();

    let mut root = BytesStart::new("Relationships");
    root.push_attribute((
        "xmlns",
        "http://schemas.openxmlformats.org/package/2006/relationships",
    ));
    writer.write_event(Event::Start(root)).ok();
    writer.write_event(Event::Text(BytesText::new("\n"))).ok();

    for rel in rels {
        let mut elem = BytesStart::new("Relationship");
        elem.push_attribute(("Id", rel.id.as_str()));
        elem.push_attribute(("Type", rel.rel_type.as_str()));
        elem.push_attribute(("Target", rel.target.as_str()));
        if rel.external {
            elem.push_attribute(("TargetMode", "External"));
        }
        writer.write_event(Event::Empty(elem)).ok();
        writer.write_event(Event::Text(BytesText::new("\n"))).ok();
    }

    writer
        .write_event(Event::End(BytesEnd::new("Relationships")))
        .ok();
    writer.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_targets_are_computed_from_the_source_part() {
        assert_eq!(
            relative_target("ppt/presentation.xml", "ppt/slides/slide1.xml"),
            "slides/slide1.xml"
        );
        assert_eq!(
            relative_target("ppt/slides/slide1.xml", "ppt/slideLayouts/slideLayout1.xml"),
            "../slideLayouts/slideLayout1.xml"
        );
        assert_eq!(
            relative_target("ppt/slides/slide1.xml", "ppt/media/image1.png"),
            "../media/image1.png"
        );
        assert_eq!(
            relative_target("", "ppt/presentation.xml"),
            "ppt/presentation.xml"
        );
        assert_eq!(
            relative_target("ppt/notesSlides/notesSlide1.xml", "ppt/slides/slide1.xml"),
            "../slides/slide1.xml"
        );
    }

    #[test]
    fn rels_part_names() {
        assert_eq!(rels_part_name(""), "_rels/.rels");
        assert_eq!(
            rels_part_name("ppt/presentation.xml"),
            "ppt/_rels/presentation.xml.rels"
        );
    }
}
