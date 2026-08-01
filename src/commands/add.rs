use std::path::Path;

use crate::dto::{AddShape, ShapeTypeInput};
use crate::engine::editor;
use crate::engine::factory;
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::opc::relationship::Relationship;
use crate::path;

pub fn execute(input: &str, path_str: &str, value: &str, output: &str) -> AppResult<()> {
    let mut pkg = Package::open(Path::new(input))?;

    let pres_part = pkg
        .get_part("ppt/presentation.xml")
        .ok_or_else(|| AppError::PartNotFound("ppt/presentation.xml".to_string()))?
        .to_vec();
    let pres_rels = pkg
        .get_rels("ppt/presentation.xml")
        .ok_or_else(|| AppError::PartNotFound("ppt/presentation.xml rels".to_string()))?;
    let mut pres = Presentation::parse(&pres_part)?;
    pres.slide_uris = pres.resolve_slide_uris(pres_rels);

    let segments = path::parse_path(path_str)?;
    let resolved = path::resolve_path(&segments)?;
    let remaining = resolved.remaining_segments();

    if remaining.is_empty() {
        // Add slide or shape — distinguish by resolved variant
        if let path::ResolvedPath::Slide { slide_idx, .. } = &resolved {
            return add_slide(&mut pkg, &mut pres, &pres_part, slide_idx, value, output);
        }
    }

    // Determine the part that owns the shapes being targeted: a slide or a
    // notes slide.
    let container_uri = match &resolved {
        path::ResolvedPath::Notes {
            slide_idx: Some(_), ..
        } => {
            return Err(AppError::PathParse(
                "Adding a notes slide is not supported; use slides[N].notes.shapes to add a shape"
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
        let new_data = editor::add_to_text_frame(&part_data, shape_idx, remaining, value)?;
        pkg.set_part(&container_uri, new_data);
    } else if remaining.len() >= 2
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        let shape_idx = resolved.shape_index()?;
        let new_data = editor::add_to_table(&part_data, shape_idx, remaining, value)?;
        pkg.set_part(&container_uri, new_data);
    } else if remaining.is_empty() {
        // Add shape
        let mut add_shape: AddShape = serde_json::from_str(value)
            .map_err(|e| AppError::InvalidValue(format!("Invalid JSON: {}", e)))?;

        // Handle image storage for picture shapes
        if add_shape.shape_type == ShapeTypeInput::Picture
            && let Some(ref img_path) = add_shape.image
        {
            let media_uri = pkg.add_image_file(img_path)?;
            let rel_target = media_uri.strip_prefix("ppt/").unwrap_or(&media_uri);
            let r_id = pkg.add_relationship(
                &container_uri,
                Relationship {
                    id: String::new(),
                    target: format!("../{}", rel_target),
                    target_mode: None,
                    rel_type:
                        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"
                            .to_string(),
                },
            );
            add_shape.image_r_id = Some(r_id);
        }

        // Handle chart part creation for chart shapes
        if add_shape.shape_type == ShapeTypeInput::Chart
            && let Some(ref chart_dto) = add_shape.chart
        {
            let chart_xml = crate::dto::xml::chart_part_to_xml(chart_dto);
            let chart_num = pkg.get_next_chart_num();
            let chart_uri = format!("ppt/charts/chart{}.xml", chart_num);
            pkg.set_part(&chart_uri, chart_xml.into_bytes());
            pkg.add_content_type_override(
                &format!("/{}", chart_uri),
                "application/vnd.openxmlformats-officedocument.drawingml.chart+xml",
            )?;
            let rel_target = chart_uri.strip_prefix("ppt/").unwrap_or(&chart_uri);
            let r_id = pkg.add_relationship(
                &container_uri,
                Relationship {
                    id: String::new(),
                    target: format!("../{}", rel_target),
                    target_mode: None,
                    rel_type:
                        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart"
                            .to_string(),
                },
            );
            add_shape.chart_r_id = Some(r_id);
        }

        let max_id = factory::find_max_shape_id(&part_data);
        let new_id = add_shape.shape_id.unwrap_or(max_id + 1);

        let new_shape_xml = factory::generate_shape_xml(&add_shape, new_id)?;

        let shape_idx = match resolved.shape_index() {
            Ok(idx) => idx,
            Err(_) => max_id as usize,
        };

        let new_data = editor::insert_shape_after(&part_data, shape_idx, &new_shape_xml)?;
        pkg.set_part(&container_uri, new_data);
    } else {
        return Err(AppError::PathParse(format!(
            "add does not support path: {}",
            path_str
        )));
    }

    pkg.save(Path::new(output))?;

    Ok(())
}

fn add_slide(
    pkg: &mut Package,
    pres: &mut Presentation,
    pres_part: &[u8],
    slide_idx: &Option<usize>,
    value: &str,
    output: &str,
) -> AppResult<()> {
    let slide_dto: crate::dto::SlideDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid slide JSON: {}", e)))?;

    if pres.slide_uris.is_empty() {
        return Err(AppError::InvalidValue(
            "Presentation has no slides to reference for layout".to_string(),
        ));
    }

    let slide_num = pkg.get_next_slide_num();
    let new_slide_uri = format!("ppt/slides/slide{}.xml", slide_num);

    let insert_pos = slide_idx.unwrap_or(pres.slide_uris.len().saturating_sub(1));
    let ref_slide_idx = insert_pos.min(pres.slide_uris.len().saturating_sub(1));
    let ref_slide_uri = &pres.slide_uris[ref_slide_idx];

    let ref_slide_rels = pkg
        .get_rels(ref_slide_uri)
        .ok_or_else(|| AppError::PartNotFound(format!("{} rels", ref_slide_uri)))?;

    let layout_rel = ref_slide_rels
        .values()
        .find(|r| r.rel_type.contains("slideLayout"))
        .ok_or_else(|| {
            AppError::InvalidValue(
                "No slide layout relationship found in reference slide".to_string(),
            )
        })?;

    let layout_target = layout_rel.target.clone();
    let layout_target_mode = layout_rel.target_mode.clone();
    let layout_rel_type = layout_rel.rel_type.clone();

    // Add relationship from presentation to new slide
    let new_r_id = pkg.add_relationship(
        "ppt/presentation.xml",
        Relationship {
            id: String::new(),
            target: format!("slides/slide{}.xml", slide_num),
            target_mode: None,
            rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide"
                .to_string(),
        },
    );

    // Add relationship from new slide to layout
    pkg.add_relationship(
        &new_slide_uri,
        Relationship {
            id: String::new(),
            target: layout_target,
            target_mode: layout_target_mode,
            rel_type: layout_rel_type,
        },
    );

    // Generate slide XML
    let slide_xml = factory::generate_slide_xml(&slide_dto, "rId1", slide_num)?;

    // Update presentation.xml
    let max_sld_id = editor::find_max_sld_id(pres_part);
    let new_pres_xml =
        editor::insert_slide_into_presentation(pres_part, insert_pos, &new_r_id, max_sld_id + 1)?;

    // Store parts
    pkg.set_part(&new_slide_uri, slide_xml);
    pkg.set_part("ppt/presentation.xml", new_pres_xml);

    // Update [Content_Types].xml
    pkg.add_content_type_override(
        &format!("/ppt/slides/slide{}.xml", slide_num),
        "application/vnd.openxmlformats-officedocument.presentationml.slide+xml",
    )?;

    pkg.save(Path::new(output))?;
    Ok(())
}
