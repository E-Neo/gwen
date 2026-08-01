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

const CHART_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";

/// Duplicate any referenced parts (images, charts) and remap their r-attributes
/// in the subtree. Returns the rewritten subtree.
///
/// - Image rels: the media part is shared; only a new relationship is added to
///   the destination slide.
/// - Chart rels: the chart XML part is duplicated (with its own rels and a
///   content-type override) and a new relationship is added.
fn remap_shape_rels(
    pkg: &mut Package,
    subtree: &[u8],
    src_slide_uri: &str,
    dst_slide_uri: &str,
) -> AppResult<Vec<u8>> {
    let src_rels = pkg
        .get_rels(src_slide_uri)
        .ok_or_else(|| AppError::PartNotFound(format!("{src_slide_uri} rels")))?
        .clone();

    let mut r_id_map: HashMap<String, String> = HashMap::new();

    for (old_rid, rel) in &src_rels {
        let is_image = rel.rel_type.contains("image");
        let is_chart = rel.rel_type.contains("chart");
        if !is_image && !is_chart {
            continue;
        }

        // Only act if the subtree actually references this relationship
        let pattern = if is_image {
            format!("r:embed=\"{old_rid}\"")
        } else {
            format!("c:chart r:id=\"{old_rid}\"")
        };
        let pattern_bytes = pattern.as_bytes();
        if !subtree
            .windows(pattern_bytes.len())
            .any(|w| w == pattern_bytes)
        {
            continue;
        }

        let new_rid = if is_image {
            // Share the media part; add a relationship on the destination slide
            pkg.add_relationship(
                dst_slide_uri,
                Relationship {
                    id: String::new(),
                    target: rel.target.clone(),
                    rel_type: rel.rel_type.clone(),
                    target_mode: rel.target_mode.clone(),
                },
            )
        } else {
            // Duplicate the chart part
            let src_chart_uri = pkg
                .resolve_relationship_target(src_slide_uri, rel)
                .ok_or_else(|| AppError::PartNotFound(rel.target.clone()))?;
            let chart_data = pkg
                .get_part(&src_chart_uri)
                .ok_or_else(|| AppError::PartNotFound(src_chart_uri.clone()))?
                .to_vec();

            let new_chart_uri = format!(
                "ppt/charts/{}_copy.xml",
                rel.target.trim_end_matches(".xml")
            );
            pkg.set_part(&new_chart_uri, chart_data);
            pkg.add_content_type_override(&format!("/{new_chart_uri}"), CHART_CONTENT_TYPE)?;

            // Copy the chart's own relationships (style/colors, …)
            if let Some(chart_rels) = pkg.get_rels(&src_chart_uri).cloned() {
                pkg.set_rels(&new_chart_uri, chart_rels);
            }

            pkg.add_relationship(
                dst_slide_uri,
                Relationship {
                    id: String::new(),
                    target: new_chart_uri
                        .strip_prefix("ppt/")
                        .unwrap_or(&new_chart_uri)
                        .to_string(),
                    rel_type: rel.rel_type.clone(),
                    target_mode: rel.target_mode.clone(),
                },
            )
        };

        r_id_map.insert(old_rid.clone(), new_rid);
    }

    if r_id_map.is_empty() {
        Ok(subtree.to_vec())
    } else {
        Ok(sanitizer::replace_r_ids(subtree, &r_id_map))
    }
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

/// Resolve a path to a slide index if it names a whole slide (`slides[N]`).
/// Returns `Ok(None)` for paths that target shapes or properties.
fn resolve_slide_path(pkg: &Package, path_str: &str) -> AppResult<Option<usize>> {
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
    match &resolved {
        path::ResolvedPath::Slide {
            slide_idx: Some(i),
            remaining,
        } if remaining.is_empty() => Ok(Some(*i)),
        _ => Ok(None),
    }
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

        // Duplicate chart/image parts and remap r-attributes
        let subtree = remap_shape_rels(&mut pkg, &subtree, &src.slide_uri, &dst.slide_uri)?;

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

    // Slide-level move: reorder slides within the presentation.
    if let (Some(from_idx), Some(to_idx)) = (
        resolve_slide_path(&pkg, from_path)?,
        resolve_slide_path(&pkg, to_path)?,
    ) {
        let pres_data = pkg
            .get_part("ppt/presentation.xml")
            .ok_or(AppError::PartNotFound("ppt/presentation.xml".to_string()))?
            .to_vec();
        let new_pres = editor::reorder_slide(&pres_data, from_idx, to_idx)?;
        pkg.set_part("ppt/presentation.xml", new_pres);
        pkg.save(Path::new(output))?;
        return Ok(());
    }

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

        // Duplicate chart/image parts and remap r-attributes
        let subtree = remap_shape_rels(&mut pkg, &subtree, &src.slide_uri, &dst.slide_uri)?;

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
