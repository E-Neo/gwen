use std::collections::HashMap;
use std::path::Path;

use crate::engine::editor;
use crate::engine::factory;
use crate::engine::sanitizer;
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::path;

fn resolve_slide_shape(pkg: &Package, path_str: &str) -> AppResult<(String, usize)> {
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

    Ok((slide_uri, shape_idx))
}

pub fn copy_shape(input: &str, from_path: &str, to_path: &str, output: &str) -> AppResult<()> {
    let mut pkg = Package::open(Path::new(input))?;
    let (src_uri, src_idx) = resolve_slide_shape(&pkg, from_path)?;
    let (dst_uri, dst_idx) = resolve_slide_shape(&pkg, to_path)?;

    let src_data = pkg
        .get_part(&src_uri)
        .ok_or_else(|| AppError::PartNotFound(src_uri.clone()))?
        .to_vec();

    let (subtree, _orig_id) = editor::extract_shape_subtree(&src_data, src_idx)?;

    let dst_data = pkg
        .get_part(&dst_uri)
        .ok_or_else(|| AppError::PartNotFound(dst_uri.clone()))?
        .to_vec();

    let max_id = factory::find_max_shape_id(&dst_data);
    let new_id = max_id + 1;

    let r_id_map = HashMap::new();
    let sanitized = sanitizer::sanitize_subtree(&subtree, new_id, &r_id_map)?;

    let new_dst_data = editor::insert_shape_after(&dst_data, dst_idx, &sanitized)?;
    pkg.set_part(&dst_uri, new_dst_data);
    pkg.save(Path::new(output))?;

    Ok(())
}

pub fn move_shape(input: &str, from_path: &str, to_path: &str, output: &str) -> AppResult<()> {
    let mut pkg = Package::open(Path::new(input))?;
    let (src_uri, src_idx) = resolve_slide_shape(&pkg, from_path)?;
    let (dst_uri, dst_idx) = resolve_slide_shape(&pkg, to_path)?;

    let src_data = pkg
        .get_part(&src_uri)
        .ok_or_else(|| AppError::PartNotFound(src_uri.clone()))?
        .to_vec();

    let (subtree, _orig_id) = editor::extract_shape_subtree(&src_data, src_idx)?;

    // Remove source
    let src_after_remove = editor::remove_shape(&src_data, src_idx)?;
    pkg.set_part(&src_uri, src_after_remove);

    // Insert at destination
    let dst_data = pkg
        .get_part(&dst_uri)
        .ok_or_else(|| AppError::PartNotFound(dst_uri.clone()))?
        .to_vec();

    let max_id = factory::find_max_shape_id(&dst_data);
    let new_id = max_id + 1;

    let r_id_map = HashMap::new();
    let sanitized = sanitizer::sanitize_subtree(&subtree, new_id, &r_id_map)?;

    let new_dst_data = editor::insert_shape_after(&dst_data, dst_idx, &sanitized)?;
    pkg.set_part(&dst_uri, new_dst_data);
    pkg.save(Path::new(output))?;

    Ok(())
}
