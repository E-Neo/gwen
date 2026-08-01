use std::path::Path;

use crate::engine::editor;
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::path;

pub fn execute(input: &str, path_str: &str, output: &str) -> AppResult<()> {
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

    // Slide-level removal: p.slides[N]
    if remaining.is_empty()
        && let path::ResolvedPath::Slide {
            slide_idx: Some(idx),
            ..
        } = &resolved
    {
        let slide_uri = pres
            .slide_uris
            .get(*idx)
            .ok_or(AppError::SlideIndexOutOfBounds(*idx))?;
        // Resolve an attached notes slide before its relationships are removed.
        let notes_uri = crate::model::notes::resolve_notes_uri(&pkg, slide_uri);
        let notes_rel_id = pkg.get_rels(slide_uri).and_then(|rels| {
            rels.values()
                .find(|r| r.rel_type == crate::model::notes::NOTES_SLIDE_REL_TYPE)
                .map(|r| r.id.clone())
        });
        let (new_pres_xml, removed_r_id) = editor::remove_slide_from_presentation(pres_part, *idx)?;
        pkg.set_part("ppt/presentation.xml", new_pres_xml);
        pkg.remove_relationship("ppt/presentation.xml", &removed_r_id);
        pkg.remove_part(slide_uri);
        pkg.remove_all_relationships(slide_uri);
        pkg.remove_content_type_override(&format!("/{}", slide_uri))?;
        if let (Some(notes_uri), Some(notes_rel_id)) = (notes_uri, notes_rel_id) {
            pkg.remove_relationship(slide_uri, &notes_rel_id);
            pkg.remove_part(&notes_uri);
            pkg.remove_all_relationships(&notes_uri);
            pkg.remove_content_type_override(&format!("/{}", notes_uri))?;
        }
        pkg.save(Path::new(output))?;
        return Ok(());
    }

    // Determine the part that owns the shapes being targeted: a slide or a
    // notes slide.
    let container_uri = match &resolved {
        path::ResolvedPath::Notes {
            slide_idx: Some(_), ..
        } => {
            return Err(AppError::PathParse(
                "Removing a whole notes slide is not supported; use slides[N].notes.shapes[M]"
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
        path::ResolvedPath::NotesShape { slide_idx, .. } => {
            let slide_uri = pres
                .slide_uris
                .get(*slide_idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*slide_idx))?;
            crate::model::notes::resolve_notes_uri(&pkg, slide_uri)
                .ok_or_else(|| AppError::PathParse("Slide has no notes slide".to_string()))?
        }
        _ => {
            let slide_idx = resolved.slide_index()?;
            pres.slide_uris
                .get(slide_idx)
                .ok_or(AppError::SlideIndexOutOfBounds(slide_idx))?
                .clone()
        }
    };

    let part_data = pkg
        .get_part(&container_uri)
        .ok_or_else(|| AppError::PartNotFound(container_uri.clone()))?
        .to_vec();

    if remaining.len() >= 2
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "text_frame")
    {
        let shape_idx = resolved.shape_index()?;
        let new_data = editor::remove_from_text_frame(&part_data, shape_idx, remaining)?;
        pkg.set_part(&container_uri, new_data);
    } else if remaining.len() >= 3
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart")
        && matches!(&remaining[1], path::PathSegment::Field(n) if n == "series")
        && matches!(&remaining[2], path::PathSegment::Index(_))
    {
        let shape_idx = resolved.shape_index()?;
        let chart_uri = crate::model::parts::resolve_chart_part(&pkg, &container_uri, shape_idx)?;
        let chart_data = pkg
            .get_part(&chart_uri)
            .ok_or_else(|| AppError::PartNotFound(chart_uri.clone()))?
            .to_vec();
        let new_data =
            crate::engine::xml_edit::remove_chart_series_lossless(&chart_data, remaining)?;
        pkg.set_part(&chart_uri, new_data);
    } else if matches!(
        &remaining,
        [
            path::PathSegment::Field(t),
            path::PathSegment::Field(r),
            path::PathSegment::Index(_),
        ] if t == "table" && (r == "rows" || r == "grid")
    ) {
        let shape_idx = resolved.shape_index()?;
        let new_data = match &remaining[1] {
            path::PathSegment::Field(n) if n == "rows" => {
                crate::engine::xml_edit::remove_table_row_lossless(
                    &part_data, shape_idx, remaining,
                )?
            }
            path::PathSegment::Field(n) if n == "grid" => {
                crate::engine::xml_edit::remove_table_column_lossless(
                    &part_data, shape_idx, remaining,
                )?
            }
            _ => unreachable!(),
        };
        pkg.set_part(&container_uri, new_data);
    } else if remaining.len() >= 2
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        let shape_idx = resolved.shape_index()?;
        let new_data = editor::remove_from_table(&part_data, shape_idx, remaining)?;
        pkg.set_part(&container_uri, new_data);
    } else if remaining.is_empty() {
        let shape_idx = resolved.shape_index()?;
        let new_data = editor::remove_shape(&part_data, shape_idx)?;
        pkg.set_part(&container_uri, new_data);
    } else {
        return Err(AppError::PathParse(format!(
            "remove does not support path: {}",
            path_str
        )));
    }

    pkg.save(Path::new(output))?;

    Ok(())
}
