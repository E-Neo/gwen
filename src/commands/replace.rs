use std::path::Path;

use crate::engine::editor;
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::path;

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

    let slide_idx = resolved.slide_index()?;
    let slide_uri = pres
        .slide_uris
        .get(slide_idx)
        .ok_or(AppError::SlideIndexOutOfBounds(slide_idx))?;

    let shape_idx = resolved.shape_index()?;

    let remaining = resolved.remaining_segments();
    if remaining.is_empty() {
        return Err(AppError::InvalidValue(
            "Path must specify a property to replace (e.g. text_frame.paragraphs[0].runs[0].text)"
                .to_string(),
        ));
    }

    let part_data = pkg
        .get_part(slide_uri)
        .ok_or(AppError::PartNotFound(slide_uri.to_string()))?
        .to_vec();

    let is_text_target = remaining.len() == 1
        && matches!(&remaining[0], path::PathSegment::Field(name) if name == "text");

    let new_data = if is_text_target {
        editor::replace_shape_text(&part_data, shape_idx, "text", value)?
    } else {
        editor::replace_shape_property(&part_data, shape_idx, remaining, value)?
    };
    pkg.set_part(slide_uri, new_data);
    pkg.save(Path::new(output))?;

    Ok(())
}
