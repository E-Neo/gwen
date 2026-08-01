use std::path::Path;

use crate::engine::editor;
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::path;

/// Route a `shapes[N]…` path to a generic part that owns a shape tree (a slide
/// master or layout). `remaining` starts with `shapes`, then the shape index.
fn replace_in_shapes_part(
    pkg: &mut Package,
    part_uri: &str,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<()> {
    let (shape_idx, rest) = match remaining {
        [
            path::PathSegment::Field(n),
            path::PathSegment::Index(i),
            rest @ ..,
        ] if n == "shapes" => (*i, rest),
        _ => {
            return Err(AppError::PathParse(
                "Expected shapes[N] after the master/layout index".to_string(),
            ));
        }
    };
    replace_shape_properties(pkg, part_uri, shape_idx, rest, value)
}

/// Resolve the chart part URI from a shape's chart r:id.
fn resolve_chart_part(pkg: &Package, slide_uri: &str, shape_idx: usize) -> AppResult<String> {
    crate::model::parts::resolve_chart_part(pkg, slide_uri, shape_idx)
}

/// Decode a `--value` into the plain string form the editors expect.
///
/// `--value` is JSON, so `'"world"'` (a JSON string) must become `world` and
/// `1200` (a JSON number) must become `1200`. Values that are not valid JSON
/// are passed through untouched for backwards compatibility.
fn scalar_string(value: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(value) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(serde_json::Value::Number(n)) => n.to_string(),
        Ok(serde_json::Value::Bool(b)) => b.to_string(),
        Ok(serde_json::Value::Null) => String::new(),
        _ => value.to_string(),
    }
}

/// Replace a property of a single shape. `container_uri` is the part holding
/// the shape (a slide or a notes slide).
fn replace_shape_properties(
    pkg: &mut Package,
    container_uri: &str,
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<()> {
    let part_data = pkg
        .get_part(container_uri)
        .ok_or_else(|| AppError::PartNotFound(container_uri.to_string()))?
        .to_vec();

    let scalar = scalar_string(value);

    let first_seg = remaining.first().and_then(|s| {
        if let path::PathSegment::Field(name) = s {
            Some(name.as_str())
        } else {
            None
        }
    });

    const SHAPE_ATTRS: [&str; 6] = ["left", "top", "width", "height", "rotation", "name"];

    match (first_seg, remaining.len()) {
        (Some("text"), 1) => {
            let new_data = editor::replace_shape_text(&part_data, shape_idx, "text", &scalar)?;
            pkg.set_part(container_uri, new_data);
        }
        (Some(name), 1) if SHAPE_ATTRS.contains(&name) => {
            let new_data = editor::replace_shape_attr(&part_data, shape_idx, remaining, &scalar)?;
            pkg.set_part(container_uri, new_data);
        }
        (Some("text_frame"), _) => {
            let new_data = crate::engine::xml_edit::replace_shape_property_lossless(
                &part_data, shape_idx, remaining, &scalar,
            )?;
            pkg.set_part(container_uri, new_data);
        }
        (Some("table"), n) if n >= 2 => {
            let new_data = crate::engine::xml_edit::replace_table_cell_property_lossless(
                &part_data, shape_idx, remaining, &scalar,
            )?;
            pkg.set_part(container_uri, new_data);
        }
        (Some("chart"), n) if n >= 2 => {
            // Chart data lives in a separate OPC part; route through package
            let chart_part_uri = resolve_chart_part(pkg, container_uri, shape_idx)?;
            let chart_data = pkg
                .get_part(&chart_part_uri)
                .ok_or_else(|| AppError::PartNotFound(chart_part_uri.clone()))?
                .to_vec();
            let new_chart_data = crate::engine::xml_edit::replace_chart_property_lossless(
                &chart_data,
                remaining,
                &scalar,
            )?;
            pkg.set_part(&chart_part_uri, new_chart_data);
        }
        (Some("crop"), n) if n >= 2 => {
            let new_data = crate::engine::xml_edit::replace_picture_crop(
                &part_data, shape_idx, remaining, &scalar,
            )?;
            pkg.set_part(container_uri, new_data);
        }
        _ => {
            // Fallback: lossy txBody round-trip for unknown paths
            let new_data = editor::replace_shape_property(&part_data, shape_idx, remaining, value)?;
            pkg.set_part(container_uri, new_data);
        }
    }
    Ok(())
}

pub fn execute(input: &str, path_str: &str, value: &str, output: &str) -> AppResult<()> {
    let mut pkg = Package::open(Path::new(input))?;

    let pres_part = pkg
        .get_part("ppt/presentation.xml")
        .ok_or(AppError::PartNotFound("ppt/presentation.xml".to_string()))?;
    let pres_rels = pkg
        .get_rels("ppt/presentation.xml")
        .ok_or(AppError::PartNotFound(
            "ppt/presentation.xml rels".to_string(),
        ))?;
    let mut pres = Presentation::parse(pres_part)?;
    pres.slide_uris = pres.resolve_slide_uris(pres_rels);

    let segments = path::parse_path(path_str)?;
    let resolved = path::resolve_path(&segments)?;

    let remaining = resolved.remaining_segments();
    if remaining.is_empty() {
        return Err(AppError::InvalidValue(
            "Path must specify a property to replace (e.g. text_frame.paragraphs[0].runs[0].text)"
                .to_string(),
        ));
    }

    match &resolved {
        path::ResolvedPath::Presentation { .. } => {
            if matches!(
                remaining.first(),
                Some(path::PathSegment::Field(n)) if n == "core_properties"
            ) {
                let prop = match remaining.get(1) {
                    Some(path::PathSegment::Field(name)) => name.as_str(),
                    _ => {
                        return Err(AppError::PathParse(
                            "core_properties requires a property name (e.g. core_properties.title)"
                                .to_string(),
                        ));
                    }
                };
                let core_data = pkg
                    .get_part("docProps/core.xml")
                    .ok_or(AppError::PartNotFound("docProps/core.xml".to_string()))?
                    .to_vec();
                let scalar = scalar_string(value);
                let new_core =
                    crate::engine::xml_edit::replace_core_property(&core_data, prop, &scalar)?;
                pkg.set_part("docProps/core.xml", new_core);
            } else {
                let pres_data = pkg
                    .get_part("ppt/presentation.xml")
                    .ok_or(AppError::PartNotFound("ppt/presentation.xml".to_string()))?
                    .to_vec();
                let new_data = editor::replace_presentation_property(&pres_data, remaining, value)?;
                pkg.set_part("ppt/presentation.xml", new_data);
            }
        }
        path::ResolvedPath::Slide {
            slide_idx: Some(slide_idx),
            remaining: _,
        } => {
            let slide_uri = pres
                .slide_uris
                .get(*slide_idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*slide_idx))?;
            let part_data = pkg
                .get_part(slide_uri)
                .ok_or(AppError::PartNotFound(slide_uri.to_string()))?
                .to_vec();
            // For now, route to shape-level edit with shape_idx 0 for slide-level properties
            let first_prop = match remaining.first() {
                Some(path::PathSegment::Field(name)) => name.as_str(),
                _ => return Err(AppError::PathParse("Expected field name".to_string())),
            };
            let attr_attrs = ["name", "left", "top", "width", "height", "rotation"];
            if attr_attrs.contains(&first_prop) {
                let scalar = scalar_string(value);
                let new_data = editor::replace_shape_attr(&part_data, 0, remaining, &scalar)?;
                pkg.set_part(slide_uri, new_data);
            } else if first_prop == "background" {
                match remaining.get(1).and_then(|s| {
                    if let path::PathSegment::Field(name) = s {
                        Some(name.as_str())
                    } else {
                        None
                    }
                }) {
                    Some("fill") => match remaining.get(2).and_then(|s| {
                        if let path::PathSegment::Field(name) = s {
                            Some(name.as_str())
                        } else {
                            None
                        }
                    }) {
                        Some("color") => {
                            let scalar = scalar_string(value);
                            let new_data =
                                crate::engine::xml_edit::set_slide_background(&part_data, &scalar)?;
                            pkg.set_part(slide_uri, new_data);
                        }
                        other => {
                            return Err(AppError::PathParse(format!(
                                "Unsupported background property '{other:?}'"
                            )));
                        }
                    },
                    other => {
                        return Err(AppError::PathParse(format!(
                            "Unsupported background path '{other:?}'"
                        )));
                    }
                }
            } else {
                return Err(AppError::PathParse(format!(
                    "Unsupported slide property '{first_prop}'"
                )));
            }
        }
        path::ResolvedPath::Slide {
            slide_idx: None, ..
        } => {
            return Err(AppError::PathParse(
                "Slide index required (e.g. p.slides[0])".to_string(),
            ));
        }
        path::ResolvedPath::Shape {
            slide_idx,
            shape_idx: Some(shape_idx),
            ..
        } => {
            let slide_uri = pres
                .slide_uris
                .get(*slide_idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*slide_idx))?;
            replace_shape_properties(&mut pkg, slide_uri, *shape_idx, remaining, value)?;
        }
        path::ResolvedPath::Shape {
            shape_idx: None, ..
        } => {
            return Err(AppError::PathParse(
                "Shape index required (e.g. p.slides[0].shapes[0])".to_string(),
            ));
        }
        path::ResolvedPath::Theme { remaining } => {
            let theme_uri = crate::model::parts::theme_uri(&pkg)
                .ok_or_else(|| AppError::PathParse("Presentation has no theme".to_string()))?;
            let theme_data = pkg
                .get_part(&theme_uri)
                .ok_or(AppError::PartNotFound(theme_uri.clone()))?
                .to_vec();
            let scalar = scalar_string(value);
            let new_data =
                crate::engine::xml_edit::replace_theme_property(&theme_data, remaining, &scalar)?;
            pkg.set_part(&theme_uri, new_data);
        }
        path::ResolvedPath::Master {
            master_idx: Some(idx),
            remaining,
        } => {
            let masters = crate::model::parts::master_uris(&pkg);
            let master_uri = masters
                .get(*idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*idx))?
                .clone();
            replace_in_shapes_part(&mut pkg, &master_uri, remaining, value)?;
        }
        path::ResolvedPath::Layout {
            layout_idx: Some(idx),
            remaining,
        } => {
            let layouts = crate::model::parts::layout_uris(&pkg);
            let layout_uri = layouts
                .get(*idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*idx))?
                .clone();
            replace_in_shapes_part(&mut pkg, &layout_uri, remaining, value)?;
        }
        path::ResolvedPath::Master { .. } | path::ResolvedPath::Layout { .. } => {
            return Err(AppError::PathParse(
                "Index required (e.g. p.slideMasters[0].shapes[0])".to_string(),
            ));
        }
        path::ResolvedPath::Notes {
            slide_idx: Some(_), ..
        } => {
            return Err(AppError::PathParse(
                "Notes-level property replacement not supported; use slides[N].notes.shapes[M]"
                    .to_string(),
            ));
        }
        path::ResolvedPath::Notes {
            slide_idx: None, ..
        } => {
            return Err(AppError::PathParse(
                "Slide index required (e.g. p.slides[0].notes)".to_string(),
            ));
        }
        path::ResolvedPath::NotesShape {
            slide_idx,
            shape_idx: Some(shape_idx),
            ..
        } => {
            let slide_uri = pres
                .slide_uris
                .get(*slide_idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*slide_idx))?;
            let notes_uri = crate::model::notes::resolve_notes_uri(&pkg, slide_uri)
                .ok_or_else(|| AppError::PathParse("Slide has no notes slide".to_string()))?;
            replace_shape_properties(&mut pkg, &notes_uri, *shape_idx, remaining, value)?;
        }
        path::ResolvedPath::NotesShape {
            shape_idx: None, ..
        } => {
            return Err(AppError::PathParse(
                "Shape index required (e.g. p.slides[0].notes.shapes[0])".to_string(),
            ));
        }
    }

    pkg.save(Path::new(output))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::scalar_string;

    #[test]
    fn decodes_json_strings() {
        assert_eq!(scalar_string("\"world\""), "world");
        assert_eq!(scalar_string("\"a & b\""), "a & b");
    }

    #[test]
    fn decodes_json_numbers_and_bools() {
        assert_eq!(scalar_string("1200"), "1200");
        assert_eq!(scalar_string("42.5"), "42.5");
        assert_eq!(scalar_string("true"), "true");
        assert_eq!(scalar_string("null"), "");
    }

    #[test]
    fn passes_through_non_json() {
        assert_eq!(scalar_string("plain text"), "plain text");
        assert_eq!(scalar_string(""), "");
    }
}
