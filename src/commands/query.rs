use std::collections::HashMap;
use std::path::Path;

use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::model::slide;
use crate::opc::Package;
use crate::path;

fn build_image_map(
    rels: Option<&HashMap<String, crate::opc::Relationship>>,
    media_dir: Option<&str>,
) -> HashMap<String, String> {
    let mut image_map = HashMap::new();
    if let Some(rels) = rels {
        for (r_id, rel) in rels {
            if rel.rel_type.contains("image") {
                let image_path = if let Some(dir) = media_dir {
                    Path::new(dir)
                        .join(&rel.target)
                        .to_string_lossy()
                        .to_string()
                } else {
                    rel.target.clone()
                };
                image_map.insert(r_id.clone(), image_path);
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
    let image_map = build_image_map(rels, media_dir);
    let shapes = slide::parse_slide_shapes(part_data, &image_map)?;

    if let Some(dir) = media_dir
        && let Some(rels) = rels
    {
        for rel in rels.values() {
            if rel.rel_type.contains("image")
                && let Some(data) = pkg.get_part(&format!("ppt/{}", rel.target))
            {
                let target_path = Path::new(dir).join(&rel.target);
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target_path, data)?;
            }
        }
    }

    Ok(shapes.into_iter().map(|s| json!(s)).collect())
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

pub fn execute(input: &str, path_str: &str, media_dir: Option<&str>) -> AppResult<()> {
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
                .map(|uri| {
                    let shapes = parse_shapes(&pkg, uri, media_dir)?;
                    Ok(json!({ "shapes": shapes }))
                })
                .collect::<AppResult<Vec<_>>>()?;
            json!({
                "slide_width": pres.slide_width,
                "slide_height": pres.slide_height,
                "slides": slides,
            })
        }
        path::ResolvedPath::Slide { slide_idx, .. } => match slide_idx {
            None => {
                let slides = pres
                    .slide_uris
                    .iter()
                    .map(|uri| {
                        let shapes = parse_shapes(&pkg, uri, media_dir)?;
                        Ok(json!({ "shapes": shapes }))
                    })
                    .collect::<AppResult<Vec<_>>>()?;
                json!(slides)
            }
            Some(idx) => {
                let uri = pres
                    .slide_uris
                    .get(*idx)
                    .ok_or(AppError::SlideIndexOutOfBounds(*idx))?;
                let shapes = parse_shapes(&pkg, uri, media_dir)?;
                json!({ "shapes": shapes })
            }
        },
        path::ResolvedPath::Shape {
            slide_idx,
            shape_idx,
            ..
        } => {
            let uri = pres
                .slide_uris
                .get(*slide_idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*slide_idx))?;
            let shapes = parse_shapes(&pkg, uri, media_dir)?;
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
    println!("{}", serde_json::to_string_pretty(&result)?);

    Ok(())
}
