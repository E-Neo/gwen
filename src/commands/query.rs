use std::collections::HashMap;
use std::path::Path;

use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::model::core_props;
use crate::model::notes;
use crate::model::presentation::Presentation;
use crate::model::slide;
use crate::opc::Package;
use crate::path;

fn build_image_map(
    rels: Option<&HashMap<String, crate::opc::Relationship>>,
) -> HashMap<String, String> {
    let mut image_map = HashMap::new();
    if let Some(rels) = rels {
        for (r_id, rel) in rels {
            if rel.rel_type.contains("image") {
                let filename = Path::new(&rel.target)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| rel.target.clone());
                image_map.insert(r_id.clone(), filename);
            }
        }
    }
    image_map
}

fn parse_shapes(
    pkg: &Package,
    uri: &str,
    media_dir: Option<&str>,
) -> AppResult<Vec<serde_json::Value>> {
    let part_data = pkg
        .get_part(uri)
        .ok_or_else(|| AppError::PartNotFound(uri.to_string()))?;
    let rels = pkg.get_rels(uri);
    let image_map = build_image_map(rels);
    let shapes = slide::parse_slide_shapes(part_data, &image_map)?;

    if let Some(dir) = media_dir
        && let Some(rels) = rels
    {
        for rel in rels.values() {
            if rel.rel_type.contains("image")
                && let Some(data) = pkg.get_part(&format!("ppt/{}", rel.target))
            {
                let filename = Path::new(&rel.target)
                    .file_name()
                    .unwrap_or(rel.target.as_ref());
                let target_path = Path::new(dir).join(filename);
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target_path, data)?;
            }
        }
    }

    Ok(shapes.into_iter().map(|s| json!(s)).collect())
}

fn slide_json(pkg: &Package, uri: &str, media_dir: Option<&str>) -> AppResult<serde_json::Value> {
    let shapes = parse_shapes(pkg, uri, media_dir)?;
    let part_data = pkg
        .get_part(uri)
        .ok_or_else(|| AppError::PartNotFound(uri.to_string()))?;
    let background = crate::engine::xml_edit::parse_slide_background(part_data);
    Ok(json!({ "shapes": shapes, "background": background }))
}

fn navigate_json(
    value: &serde_json::Value,
    segments: &[path::PathSegment],
) -> AppResult<serde_json::Value> {
    let mut current = value.clone();
    for seg in segments {
        current = match seg {
            path::PathSegment::Field(name) => match current.get(name) {
                Some(v) => v.clone(),
                None => return Err(AppError::PathParse(format!("Field '{name}' not found"))),
            },
            path::PathSegment::Index(idx) => match current.get(*idx) {
                Some(v) => v.clone(),
                None => return Err(AppError::PathParse(format!("Index {idx} out of bounds"))),
            },
        };
    }
    Ok(current)
}

pub fn execute(
    input: &str,
    path_str: &str,
    media_dir: Option<&str>,
    pretty: bool,
) -> AppResult<()> {
    let pkg = Package::open(Path::new(input))?;

    let pres_part = pkg
        .get_part("ppt/presentation.xml")
        .ok_or_else(|| AppError::PartNotFound("ppt/presentation.xml".to_string()))?;
    let pres_rels = pkg
        .get_rels("ppt/presentation.xml")
        .ok_or_else(|| AppError::PartNotFound("ppt/presentation.xml rels".to_string()))?;
    let mut pres = Presentation::parse(pres_part)?;
    pres.slide_uris = pres.resolve_slide_uris(pres_rels);

    let segments = path::parse_path(path_str)?;
    let resolved = path::resolve_path(&segments)?;

    let value = match &resolved {
        path::ResolvedPath::Presentation { .. } => {
            let slides = pres
                .slide_uris
                .iter()
                .map(|uri| slide_json(&pkg, uri, media_dir))
                .collect::<AppResult<Vec<_>>>()?;
            let core = pkg
                .get_part("docProps/core.xml")
                .map(core_props::parse_core_properties)
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            json!({
                "slide_width": pres.slide_width,
                "slide_height": pres.slide_height,
                "slides": slides,
                "core_properties": core,
            })
        }
        path::ResolvedPath::Slide { slide_idx, .. } => match slide_idx {
            None => {
                let slides = pres
                    .slide_uris
                    .iter()
                    .map(|uri| slide_json(&pkg, uri, media_dir))
                    .collect::<AppResult<Vec<_>>>()?;
                json!(slides)
            }
            Some(idx) => {
                let uri = pres
                    .slide_uris
                    .get(*idx)
                    .ok_or(AppError::SlideIndexOutOfBounds(*idx))?;
                slide_json(&pkg, uri, media_dir)?
            }
        },
        path::ResolvedPath::Shape {
            slide_idx,
            shape_idx,
            remaining,
        } => {
            let uri = pres
                .slide_uris
                .get(*slide_idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*slide_idx))?;
            let shapes = parse_shapes(&pkg, uri, media_dir)?;
            // Chart data lives in a separate OPC part; resolve and parse it.
            if let Some(path::PathSegment::Field(n)) = remaining.first()
                && n == "chart"
            {
                let idx = shape_idx
                    .ok_or_else(|| AppError::PathParse("Chart shape index required".to_string()))?;
                let empty_map = HashMap::new();
                let slide_data = pkg
                    .get_part(uri)
                    .ok_or_else(|| AppError::PartNotFound(uri.to_string()))?;
                let parsed = crate::model::slide::parse_slide_shapes(slide_data, &empty_map)?;
                let shape = parsed
                    .get(idx)
                    .ok_or(AppError::ShapeIndexOutOfBounds(idx))?;
                let r_id = shape
                    .chart
                    .as_ref()
                    .and_then(|c| c.r_id.as_ref())
                    .ok_or_else(|| {
                        AppError::PathParse("Shape has no chart relationship".to_string())
                    })?;
                let rels = pkg
                    .get_rels(uri)
                    .ok_or_else(|| AppError::PartNotFound(format!("{uri} rels")))?;
                let rel = rels.get(r_id).ok_or_else(|| {
                    AppError::PathParse("Chart relationship not found".to_string())
                })?;
                let chart_uri = pkg.resolve_relationship_target(uri, rel).ok_or_else(|| {
                    AppError::PathParse("Chart relationship target missing".to_string())
                })?;
                let chart_data = pkg
                    .get_part(&chart_uri)
                    .ok_or(AppError::PartNotFound(chart_uri.clone()))?;
                json!({ "chart": crate::model::chart::parse_chart(chart_data) })
            } else {
                match shape_idx {
                    None => json!(shapes),
                    Some(idx) => shapes
                        .get(*idx)
                        .ok_or(AppError::ShapeIndexOutOfBounds(*idx))?
                        .clone(),
                }
            }
        }
        path::ResolvedPath::Theme { remaining: _ } => {
            let theme_uri = crate::model::parts::theme_uri(&pkg)
                .ok_or_else(|| AppError::PathParse("Presentation has no theme".to_string()))?;
            let part_data = pkg
                .get_part(&theme_uri)
                .ok_or(AppError::PartNotFound(theme_uri.clone()))?;
            crate::model::theme::parse_theme(part_data)
        }
        path::ResolvedPath::Master {
            master_idx,
            remaining: _,
        } => {
            let masters = crate::model::parts::master_uris(&pkg);
            match master_idx {
                Some(i) => {
                    let m = masters.get(*i).ok_or(AppError::SlideIndexOutOfBounds(*i))?;
                    let shapes = parse_shapes(&pkg, m, None)?;
                    json!({ "shapes": shapes })
                }
                None => json!(
                    masters
                        .iter()
                        .map(|m| {
                            let shapes = parse_shapes(&pkg, m, None)?;
                            Ok(json!({ "shapes": shapes }))
                        })
                        .collect::<AppResult<Vec<_>>>()?
                ),
            }
        }
        path::ResolvedPath::Layout {
            layout_idx,
            remaining: _,
        } => {
            let layouts = crate::model::parts::layout_uris(&pkg);
            match layout_idx {
                Some(i) => {
                    let l = layouts.get(*i).ok_or(AppError::SlideIndexOutOfBounds(*i))?;
                    let shapes = parse_shapes(&pkg, l, None)?;
                    json!({ "shapes": shapes })
                }
                None => json!(
                    layouts
                        .iter()
                        .map(|l| {
                            let shapes = parse_shapes(&pkg, l, None)?;
                            Ok(json!({ "shapes": shapes }))
                        })
                        .collect::<AppResult<Vec<_>>>()?
                ),
            }
        }
        path::ResolvedPath::Notes {
            slide_idx: Some(idx),
            ..
        } => {
            let slide_uri = pres
                .slide_uris
                .get(*idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*idx))?;
            let notes_uri = notes::resolve_notes_uri(&pkg, slide_uri)
                .ok_or_else(|| AppError::PathParse("Slide has no notes slide".to_string()))?;
            let shapes = parse_shapes(&pkg, &notes_uri, media_dir)?;
            json!({ "shapes": shapes })
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
            shape_idx,
            ..
        } => {
            let slide_uri = pres
                .slide_uris
                .get(*slide_idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*slide_idx))?;
            let notes_uri = notes::resolve_notes_uri(&pkg, slide_uri)
                .ok_or_else(|| AppError::PathParse("Slide has no notes slide".to_string()))?;
            let shapes = parse_shapes(&pkg, &notes_uri, media_dir)?;
            match shape_idx {
                None => json!(shapes),
                Some(idx) => shapes
                    .get(*idx)
                    .ok_or(AppError::ShapeIndexOutOfBounds(*idx))?
                    .clone(),
            }
        }
    };

    let result = navigate_json(&value, resolved.remaining_segments())?;
    let output = if pretty {
        serde_json::to_string_pretty(&result)?
    } else {
        serde_json::to_string(&result)?
    };
    println!("{output}");

    Ok(())
}
