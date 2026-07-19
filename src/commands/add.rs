use std::path::Path;

use crate::dto::AddShape;
use crate::engine::editor;
use crate::engine::factory;
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::path;

pub fn execute(input: &str, path_str: &str, value: &str, output: &str) -> AppResult<()> {
    let mut pkg = Package::open(Path::new(input))?;

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

    let add_shape: AddShape = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid JSON: {}", e)))?;

    let slide_idx = resolved.slide_index()?;
    let slide_uri = pres
        .slide_uris
        .get(slide_idx)
        .ok_or(AppError::SlideIndexOutOfBounds(slide_idx))?;

    let part_data = pkg
        .get_part(slide_uri)
        .ok_or_else(|| AppError::PartNotFound(slide_uri.to_string()))?
        .to_vec();

    let max_id = factory::find_max_shape_id(&part_data);
    let new_id = add_shape.shape_id.unwrap_or(max_id + 1);

    let new_shape_xml = factory::generate_shape_xml(&add_shape, new_id)?;

    let shape_idx = resolved.shape_index()?;

    let new_data = editor::insert_shape_after(&part_data, shape_idx, &new_shape_xml)?;

    pkg.set_part(slide_uri, new_data);
    pkg.save(Path::new(output))?;

    Ok(())
}
