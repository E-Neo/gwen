use crate::opc::Package;

/// Relationship type linking a slide to its notes slide.
pub const NOTES_SLIDE_REL_TYPE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide";

/// Resolve the notes slide part URI for a slide, if one exists.
pub fn resolve_notes_uri(pkg: &Package, slide_uri: &str) -> Option<String> {
    let rels = pkg.get_rels(slide_uri)?;
    for rel in rels.values() {
        if rel.rel_type == NOTES_SLIDE_REL_TYPE {
            return pkg.resolve_relationship_target(slide_uri, rel);
        }
    }
    None
}
