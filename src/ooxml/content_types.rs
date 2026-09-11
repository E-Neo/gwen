//! Content-type resolution for parts.

use crate::error::Result;

pub const PRESENTATION: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
pub const SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
pub const SLIDE_MASTER: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
pub const SLIDE_LAYOUT: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
pub const THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
pub const NOTES_SLIDE: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml";
pub const NOTES_MASTER: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml";
pub const PRES_PROPS: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml";
pub const VIEW_PROPS: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.viewProps+xml";
pub const TABLE_STYLES: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.tableStyles+xml";
pub const EXTENDED: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
pub const CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";

/// The `Override` content type for a part, if it is one of the known part
/// kinds. Numbered parts (`slideN.xml`) match by prefix.
pub fn override_for(uri: &str) -> Option<&'static str> {
    let numbered = |prefix: &str| {
        uri.strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(".xml"))
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    };
    if uri == "ppt/presentation.xml" {
        return Some(PRESENTATION);
    }
    if uri == "docProps/core.xml" {
        return Some(CORE);
    }
    if uri == "docProps/app.xml" {
        return Some(EXTENDED);
    }
    if uri == "ppt/presProps.xml" {
        return Some(PRES_PROPS);
    }
    if uri == "ppt/viewProps.xml" {
        return Some(VIEW_PROPS);
    }
    if uri == "ppt/tableStyles.xml" {
        return Some(TABLE_STYLES);
    }
    if uri == "ppt/notesMasters/notesMaster1.xml" {
        return Some(NOTES_MASTER);
    }
    if numbered("ppt/slides/slide") {
        return Some(SLIDE);
    }
    if numbered("ppt/slideMasters/slideMaster") {
        return Some(SLIDE_MASTER);
    }
    if numbered("ppt/slideLayouts/slideLayout") {
        return Some(SLIDE_LAYOUT);
    }
    if numbered("ppt/notesSlides/notesSlide") {
        return Some(NOTES_SLIDE);
    }
    if numbered("ppt/theme/theme") {
        return Some(THEME);
    }
    None
}

/// The `Default` content type for a file extension, if supported.
pub fn default_for_ext(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "rels" => Some("application/vnd.openxmlformats-package.relationships+xml"),
        "xml" => Some("application/xml"),
        "png" => Some("image/png"),
        "jpeg" | "jpg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        "emf" => Some("image/x-emf"),
        "wmf" => Some("image/x-wmf"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

/// Build `[Content_Types].xml` covering every part. Any part without a known
/// override or extension default is an error — an uncovered part makes the
/// whole package invalid.
pub fn content_types_xml(part_uris: &[String]) -> Result<Vec<u8>> {
    use crate::ooxml::Elem;

    let mut defaults: Vec<(String, &'static str)> = Vec::new();
    let mut overrides: Vec<(String, &'static str)> = Vec::new();
    let mut uncovered: Vec<&str> = Vec::new();

    for uri in part_uris {
        if let Some(ct) = override_for(uri) {
            overrides.push((uri.clone(), ct));
            continue;
        }
        let ext = uri.rsplit('.').next().unwrap_or("");
        match default_for_ext(ext) {
            Some(ct) => {
                if !defaults.iter().any(|(e, _)| e.eq_ignore_ascii_case(ext)) {
                    defaults.push((ext.to_string(), ct));
                }
            }
            None => uncovered.push(uri),
        }
    }
    if !uncovered.is_empty() {
        return Err(miette::miette!(
            "no content type for: {} (unsupported file extension)",
            uncovered.join(", ")
        ));
    }

    let mut root = Elem::new("Types").attr(
        "xmlns",
        "http://schemas.openxmlformats.org/package/2006/content-types",
    );
    defaults.sort();
    for (ext, ct) in defaults {
        root = root.child(
            Elem::new("Default")
                .attr("Extension", ext)
                .attr("ContentType", ct),
        );
    }
    overrides.sort_by(|a, b| a.0.cmp(&b.0));
    for (uri, ct) in overrides {
        root = root.child(
            Elem::new("Override")
                .attr("PartName", format!("/{uri}"))
                .attr("ContentType", ct),
        );
    }
    Ok(root.to_xml())
}
