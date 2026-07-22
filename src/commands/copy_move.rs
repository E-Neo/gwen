use std::collections::HashMap;
use std::path::Path;

use crate::engine::editor;
use crate::engine::factory;
use crate::engine::sanitizer;
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::path;

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

    Ok(ResolvedTarget { slide_uri, shape_idx, remaining })
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

        let new_dst_data = editor::insert_into_txbody(&dst_data, dst.shape_idx, &dst.remaining, &subtree)?;
        pkg.set_part(&dst.slide_uri, new_dst_data);
    } else if src.remaining.is_empty() && dst.remaining.is_empty() {
        // Copy shape
        let (subtree, _orig_id) = editor::extract_shape_subtree(&src_data, src.shape_idx)?;

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

        let new_dst_data = editor::insert_into_txbody(&dst_data, dst.shape_idx, &dst.remaining, &subtree)?;
        pkg.set_part(&dst.slide_uri, new_dst_data);
    } else if src.remaining.is_empty() && dst.remaining.is_empty() {
        let (subtree, _orig_id) = editor::extract_shape_subtree(&src_data, src.shape_idx)?;

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
