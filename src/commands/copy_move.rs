use std::collections::HashMap;
use std::path::Path;

use crate::engine::editor;
use crate::engine::factory;
use crate::engine::sanitizer;
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::opc::Relationship;
use crate::path;

/// Duplicate any chart parts referenced by the shape subtree and remap r:id.
fn handle_chart_rels(
    pkg: &mut Package,
    subtree: &[u8],
    src_slide_uri: &str,
    dst_slide_uri: &str,
) -> AppResult<Vec<u8>> {
    let src_rels = pkg
        .get_rels(src_slide_uri)
        .ok_or_else(|| AppError::PartNotFound(format!("{src_slide_uri} rels")))?
        .clone();

    let mut result = subtree.to_vec();

    for (old_rid, rel) in &src_rels {
        if !rel.rel_type.contains("chart") {
            continue;
        }
        let pattern = format!("c:chart r:id=\"{old_rid}\"");
        let pattern_bytes = pattern.as_bytes();
        if !result
            .windows(pattern_bytes.len())
            .any(|w| w == pattern_bytes)
        {
            continue;
        }

        let src_chart_uri = format!("ppt/{}", rel.target);
        let chart_data = pkg
            .get_part(&src_chart_uri)
            .ok_or(AppError::PartNotFound(src_chart_uri))?
            .to_vec();

        // Create new chart part with a unique name
        let new_chart_uri = format!(
            "ppt/charts/{}_copy.xml",
            rel.target.trim_end_matches(".xml")
        );
        pkg.set_part(&new_chart_uri, chart_data);

        // Add relationship and get the new r:id
        let new_rid = {
            let new_rel = Relationship {
                id: String::new(),
                target: new_chart_uri
                    .strip_prefix("ppt/")
                    .unwrap_or(&new_chart_uri)
                    .to_string(),
                rel_type: rel.rel_type.clone(),
                target_mode: rel.target_mode.clone(),
            };
            pkg.add_relationship(dst_slide_uri, new_rel)
        };

        // Replace r:id in subtree bytes
        let old_bytes = format!("r:id=\"{old_rid}\"").into_bytes();
        let new_bytes = format!("r:id=\"{new_rid}\"").into_bytes();
        let mut new_result = Vec::with_capacity(result.len());
        let mut i = 0;
        while i < result.len() {
            if i + old_bytes.len() <= result.len() && result[i..i + old_bytes.len()] == old_bytes {
                new_result.extend_from_slice(&new_bytes);
                i += old_bytes.len();
            } else {
                new_result.push(result[i]);
                i += 1;
            }
        }
        result = new_result;
    }

    Ok(result)
}

struct ResolvedTarget {
    slide_uri: String,
    shape_idx: usize,
    remaining: Vec<path::PathSegment>,
}

fn resolve_target(pkg: &Package, path_str: &str) -> AppResult<ResolvedTarget> {
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

    let slide_idx = resolved.slide_index()?;
    let slide_uri = pres
        .slide_uris
        .get(slide_idx)
        .ok_or(AppError::SlideIndexOutOfBounds(slide_idx))?
        .clone();

    let shape_idx = resolved.shape_index()?;
    let remaining = resolved.remaining_segments().to_vec();

    Ok(ResolvedTarget {
        slide_uri,
        shape_idx,
        remaining,
    })
}

pub fn copy_shape(input: &str, from_path: &str, to_path: &str, output: &str) -> AppResult<()> {
    let mut pkg = Package::open(Path::new(input))?;
    let src = resolve_target(&pkg, from_path)?;
    let dst = resolve_target(&pkg, to_path)?;

    let src_data = pkg
        .get_part(&src.slide_uri)
        .ok_or_else(|| AppError::PartNotFound(src.slide_uri.clone()))?
        .to_vec();

    if src.remaining.len() >= 2
        && matches!(&src.remaining[0], path::PathSegment::Field(n) if n == "text_frame")
    {
        // Copy text element (paragraph or run)
        let subtree = editor::extract_txbody_element(&src_data, src.shape_idx, &src.remaining)?;

        let dst_data = pkg
            .get_part(&dst.slide_uri)
            .ok_or_else(|| AppError::PartNotFound(dst.slide_uri.clone()))?
            .to_vec();

        let new_dst_data =
            editor::insert_into_txbody(&dst_data, dst.shape_idx, &dst.remaining, &subtree)?;
        pkg.set_part(&dst.slide_uri, new_dst_data);
    } else if src.remaining.is_empty() && dst.remaining.is_empty() {
        // Copy shape
        let (subtree, _orig_id) = editor::extract_shape_subtree(&src_data, src.shape_idx)?;

        // Duplicate chart parts if the shape contains chart references
        let subtree = handle_chart_rels(&mut pkg, &subtree, &src.slide_uri, &dst.slide_uri)?;

        let dst_data = pkg
            .get_part(&dst.slide_uri)
            .ok_or_else(|| AppError::PartNotFound(dst.slide_uri.clone()))?
            .to_vec();

        let max_id = factory::find_max_shape_id(&dst_data);
        let new_id = max_id + 1;

        let r_id_map = HashMap::new();
        let sanitized = sanitizer::sanitize_subtree(&subtree, new_id, &r_id_map)?;

        let new_dst_data = editor::insert_shape_after(&dst_data, dst.shape_idx, &sanitized)?;
        pkg.set_part(&dst.slide_uri, new_dst_data);
    } else {
        return Err(AppError::PathParse(format!(
            "copy does not support path: {} / {}",
            from_path, to_path
        )));
    }

    pkg.save(Path::new(output))?;
    Ok(())
}

pub fn move_shape(input: &str, from_path: &str, to_path: &str, output: &str) -> AppResult<()> {
    let mut pkg = Package::open(Path::new(input))?;
    let src = resolve_target(&pkg, from_path)?;
    let dst = resolve_target(&pkg, to_path)?;

    let src_data = pkg
        .get_part(&src.slide_uri)
        .ok_or_else(|| AppError::PartNotFound(src.slide_uri.clone()))?
        .to_vec();

    if src.remaining.len() >= 2
        && matches!(&src.remaining[0], path::PathSegment::Field(n) if n == "text_frame")
    {
        // Move text element (paragraph or run)
        let subtree = editor::extract_txbody_element(&src_data, src.shape_idx, &src.remaining)?;

        // Remove source
        let src_after = editor::remove_from_text_frame(&src_data, src.shape_idx, &src.remaining)?;
        pkg.set_part(&src.slide_uri, src_after);

        // Insert at destination
        let dst_data = pkg
            .get_part(&dst.slide_uri)
            .ok_or_else(|| AppError::PartNotFound(dst.slide_uri.clone()))?
            .to_vec();

        let new_dst_data =
            editor::insert_into_txbody(&dst_data, dst.shape_idx, &dst.remaining, &subtree)?;
        pkg.set_part(&dst.slide_uri, new_dst_data);
    } else if src.remaining.is_empty() && dst.remaining.is_empty() {
        let (subtree, _orig_id) = editor::extract_shape_subtree(&src_data, src.shape_idx)?;

        // Duplicate chart parts if the shape contains chart references
        let subtree = handle_chart_rels(&mut pkg, &subtree, &src.slide_uri, &dst.slide_uri)?;

        let src_after_remove = editor::remove_shape(&src_data, src.shape_idx)?;
        pkg.set_part(&src.slide_uri, src_after_remove);

        let dst_data = pkg
            .get_part(&dst.slide_uri)
            .ok_or_else(|| AppError::PartNotFound(dst.slide_uri.clone()))?
            .to_vec();

        let max_id = factory::find_max_shape_id(&dst_data);
        let new_id = max_id + 1;

        let r_id_map = HashMap::new();
        let sanitized = sanitizer::sanitize_subtree(&subtree, new_id, &r_id_map)?;

        let new_dst_data = editor::insert_shape_after(&dst_data, dst.shape_idx, &sanitized)?;
        pkg.set_part(&dst.slide_uri, new_dst_data);
    } else {
        return Err(AppError::PathParse(format!(
            "move does not support path: {} / {}",
            from_path, to_path
        )));
    }

    pkg.save(Path::new(output))?;
    Ok(())
}
