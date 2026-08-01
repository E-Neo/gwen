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
    let rels = pkg
        .get_rels(slide_uri)
        .ok_or_else(|| AppError::PartNotFound(format!("{slide_uri} rels")))?;
    let rel = rels
        .get(r_id)
        .ok_or_else(|| AppError::PathParse(format!("Chart relationship {r_id} not found")))?;
    let base_dir = slide_uri.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
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

/// Resolve the slide master part URIs, in relationship order.
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
    out
}

/// Resolve all slide layout part URIs (walking each master's relationships).
pub fn layout_uris(pkg: &Package) -> Vec<String> {
    let mut out = Vec::new();
    for master in master_uris(pkg) {
        let Some(rels) = pkg.get_rels(&master) else {
            continue;
        };
        for rel in rels.values() {
            if rel.rel_type.contains("slideLayout")
                && let Some(uri) = pkg.resolve_relationship_target(&master, rel)
            {
                out.push(uri);
            }
        }
    }
    out
}
