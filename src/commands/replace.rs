use std::collections::HashMap;
use std::path::Path;

use crate::engine::editor;
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::model::slide;
use crate::opc::Package;
use crate::path;

/// Resolve the chart part URI from a shape's chart r:id.
fn resolve_chart_part(pkg: &Package, slide_uri: &str, shape_idx: usize) -> AppResult<String> {
    let empty_map: HashMap<String, String> = HashMap::new();
    let slide_data = pkg
        .get_part(slide_uri)
        .ok_or_else(|| AppError::PartNotFound(slide_uri.to_string()))?;
    let shapes = slide::parse_slide_shapes(slide_data, &empty_map)?;
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
    let chart_uri = format!("ppt/{}", rel.target);
    Ok(chart_uri)
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
            let pres_data = pkg
                .get_part("ppt/presentation.xml")
                .ok_or(AppError::PartNotFound("ppt/presentation.xml".to_string()))?
                .to_vec();
            let new_data = editor::replace_presentation_property(&pres_data, remaining, value)?;
            pkg.set_part("ppt/presentation.xml", new_data);
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
                let new_data = editor::replace_shape_attr(&part_data, 0, remaining, value)?;
                pkg.set_part(slide_uri, new_data);
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
            let part_data = pkg
                .get_part(slide_uri)
                .ok_or(AppError::PartNotFound(slide_uri.to_string()))?
                .to_vec();

            let is_attr_target = remaining.len() == 1
                && matches!(&remaining[0], path::PathSegment::Field(name)
                    if matches!(name.as_str(), "left" | "top" | "width" | "height" | "rotation" | "name"));

            let is_text_target = remaining.len() == 1
                && matches!(&remaining[0], path::PathSegment::Field(name) if name == "text");

            let is_table_cell = remaining.len() >= 2
                && matches!(&remaining[0], path::PathSegment::Field(name) if name == "table");

            let is_chart_target = remaining.len() >= 2
                && matches!(&remaining[0], path::PathSegment::Field(name) if name == "chart");

            let new_data = if is_text_target {
                editor::replace_shape_text(&part_data, *shape_idx, "text", value)?
            } else if is_attr_target {
                editor::replace_shape_attr(&part_data, *shape_idx, remaining, value)?
            } else if is_table_cell {
                editor::replace_shape_table_cell(&part_data, *shape_idx, remaining, value)?
            } else if is_chart_target {
                // Chart data lives in a separate OPC part; route through package
                let chart_part_uri = resolve_chart_part(&pkg, slide_uri, *shape_idx)?;
                let chart_data = pkg
                    .get_part(&chart_part_uri)
                    .ok_or_else(|| AppError::PartNotFound(chart_part_uri.clone()))?
                    .to_vec();
                let new_chart_data = editor::replace_shape_chart(&chart_data, remaining, value)?;
                pkg.set_part(&chart_part_uri, new_chart_data);
                // Return original slide data unchanged
                part_data.clone()
            } else {
                editor::replace_shape_property(&part_data, *shape_idx, remaining, value)?
            };
            pkg.set_part(slide_uri, new_data);
        }
        path::ResolvedPath::Shape {
            shape_idx: None, ..
        } => {
            return Err(AppError::PathParse(
                "Shape index required (e.g. p.slides[0].shapes[0])".to_string(),
            ));
        }
    }

    pkg.save(Path::new(output))?;
    Ok(())
}
