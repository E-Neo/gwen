use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::{AppError, AppResult};
use crate::opc::Package;

/// Resolve the chart part URI from a shape's chart r:id.
pub fn resolve_chart_part(pkg: &Package, slide_uri: &str, shape_idx: usize) -> AppResult<String> {
    let empty_map = std::collections::HashMap::new();
    let slide_data = pkg
        .get_part(slide_uri)
        .ok_or_else(|| AppError::PartNotFound(slide_uri.to_string()))?;
    let shapes = crate::model::slide::parse_slide_shapes(slide_data, &empty_map)?;
    let shape = shapes
        .get(shape_idx)
        .ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let r_id = shape
        .chart
        .as_ref()
        .and_then(|c| c.r_id.as_ref())
        .ok_or_else(|| AppError::PathParse("Shape has no chart relationship".to_string()))?;
    resolve_chart_part_by_rid(pkg, slide_uri, r_id)
}

/// Resolve a chart part URI from a relationship id on a container part.
pub fn resolve_chart_part_by_rid(
    pkg: &Package,
    container_uri: &str,
    r_id: &str,
) -> AppResult<String> {
    let rels = pkg
        .get_rels(container_uri)
        .ok_or_else(|| AppError::PartNotFound(format!("{container_uri} rels")))?;
    let rel = rels
        .get(r_id)
        .ok_or_else(|| AppError::PathParse(format!("Chart relationship {r_id} not found")))?;
    let base_dir = container_uri.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let rel_path = rel.target.trim_start_matches('/');
    let mut parts: Vec<&str> = base_dir.split('/').collect();
    for seg in rel_path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(seg),
        }
    }
    Ok(parts.join("/"))
}

/// Resolve the theme part URI (e.g. `ppt/theme/theme1.xml`), if present.
pub fn theme_uri(pkg: &Package) -> Option<String> {
    let rels = pkg.get_rels("ppt/presentation.xml")?;
    for rel in rels.values() {
        if rel.rel_type.contains("theme") {
            return pkg.resolve_relationship_target("ppt/presentation.xml", rel);
        }
    }
    None
}

/// Resolve the slide master part URIs, deterministically sorted by URI.
pub fn master_uris(pkg: &Package) -> Vec<String> {
    let mut out = Vec::new();
    let Some(rels) = pkg.get_rels("ppt/presentation.xml") else {
        return out;
    };
    for rel in rels.values() {
        if rel.rel_type.contains("slideMaster")
            && let Some(uri) = pkg.resolve_relationship_target("ppt/presentation.xml", rel)
        {
            out.push(uri);
        }
    }
    out.sort();
    out
}

/// Resolve the slide layout part URIs owned by a master, sorted by URI.
pub fn master_slide_layout_uris(pkg: &Package, master_uri: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some(rels) = pkg.get_rels(master_uri) else {
        return out;
    };
    for rel in rels.values() {
        if rel.rel_type.contains("slideLayout")
            && let Some(uri) = pkg.resolve_relationship_target(master_uri, rel)
        {
            out.push(uri);
        }
    }
    out.sort();
    out
}

/// Resolve the slide layout part URI referenced by a slide, if any.
pub fn slide_layout_uri(pkg: &Package, slide_uri: &str) -> Option<String> {
    let rels = pkg.get_rels(slide_uri)?;
    for rel in rels.values() {
        if rel.rel_type.contains("slideLayout") {
            return pkg.resolve_relationship_target(slide_uri, rel);
        }
    }
    None
}

/// Resolve which master owns a slide's layout, returning `(master_idx, layout_idx)`
/// into `master_uris` and `master_slide_layout_uris` respectively.
pub fn slide_layout_ref(pkg: &Package, slide_uri: &str) -> Option<(usize, usize)> {
    let layout_uri = slide_layout_uri(pkg, slide_uri)?;
    for (m, master) in master_uris(pkg).iter().enumerate() {
        if let Some(l) = master_slide_layout_uris(pkg, master)
            .iter()
            .position(|u| u == &layout_uri)
        {
            return Some((m, l));
        }
    }
    None
}

/// Read the `name` attribute of a slide/master/layout's `p:cSld` element
/// (python-pptx `_BaseSlide.name`).
pub fn c_sld_name(pkg: &Package, uri: &str) -> Option<String> {
    let data = pkg.get_part(uri)?;
    let mut reader = Reader::from_reader(data);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"p:cSld" => {
                return e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == b"name")
                    .map(|a| String::from_utf8_lossy(&a.value).to_string());
            }
            Ok(Event::Eof) => return None,
            Err(_) => return None,
            _ => {}
        }
    }
}
