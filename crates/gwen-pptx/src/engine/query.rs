use std::collections::HashMap;
use std::path::Path;

use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::model::core_props;
use crate::model::notes;
use crate::model::presentation::Presentation;
use crate::model::slide;
use crate::opc::{Package, Relationship};
use crate::path;

/// Resolve every run hyperlink's `r_id` relationship into a self-contained
/// `address`, then drop the relationship scaffolding. This is what makes the
/// mirror's `[text](url)` form lossless: an external relationship becomes its
/// URL, an internal slide relationship becomes `slide://N`, and any other
/// internal target becomes `internal://<part uri>`. Links that resolve to
/// nothing are dropped.
fn resolve_run_hyperlinks(
    pkg: &Package,
    source_uri: &str,
    rels: Option<&HashMap<String, Relationship>>,
    shapes: &mut [crate::dto::ShapeDto],
) {
    for shape in shapes {
        if let Some(tf) = &mut shape.text_frame {
            for para in &mut tf.paragraphs {
                for run in &mut para.runs {
                    resolve_run_hyperlink(pkg, source_uri, rels, run);
                }
            }
        }
        if let Some(table) = &mut shape.table {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    if let Some(tf) = &mut cell.text_frame {
                        for para in &mut tf.paragraphs {
                            for run in &mut para.runs {
                                resolve_run_hyperlink(pkg, source_uri, rels, run);
                            }
                        }
                    }
                }
            }
        }
        if let Some(children) = &mut shape.shapes {
            resolve_run_hyperlinks(pkg, source_uri, rels, children);
        }
    }
}

fn resolve_run_hyperlink(
    pkg: &Package,
    source_uri: &str,
    rels: Option<&HashMap<String, Relationship>>,
    run: &mut crate::dto::RunDto,
) {
    let Some(hyperlink) = &mut run.hyperlink else {
        return;
    };
    if hyperlink.address.is_none()
        && let Some(r_id) = &hyperlink.r_id
        && let Some(rel) = rels.and_then(|rels| rels.get(r_id))
    {
        if rel.target_mode.as_deref() == Some("External") {
            hyperlink.address = Some(rel.target.clone());
        } else if let Some(target_uri) = pkg.resolve_relationship_target(source_uri, rel) {
            hyperlink.address = Some(match slide_index_from_uri(&target_uri) {
                Some(n) => format!("slide://{n}"),
                None => format!("internal://{target_uri}"),
            });
        }
    }
    if hyperlink.address.is_some() {
        hyperlink.tooltip = None;
        hyperlink.r_id = None;
    } else {
        run.hyperlink = None;
    }
}

fn slide_index_from_uri(uri: &str) -> Option<usize> {
    uri.strip_prefix("ppt/slides/slide")?
        .strip_suffix(".xml")?
        .parse()
        .ok()
}

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
    let mut shapes = slide::parse_slide_shapes(part_data, &image_map)?;
    resolve_run_hyperlinks(pkg, uri, rels, &mut shapes);
    crate::model::placeholder::resolve_placeholder_properties(pkg, uri, &mut shapes)?;

    // Chart data lives in separate OPC parts; resolve each chart shape's part
    // and merge the full chart (type + series) into the shape JSON so query
    // output round-trips through `build`.
    for shape in &mut shapes {
        if shape.chart.is_some() {
            let r_id = shape.chart.as_ref().and_then(|c| c.r_id.as_ref()).cloned();
            if let Some(r_id) = r_id
                && let Ok(chart_uri) =
                    crate::model::parts::resolve_chart_part_by_rid(pkg, uri, &r_id)
                && let Some(chart_data) = pkg.get_part(&chart_uri)
            {
                let chart_json = crate::model::chart::parse_chart(chart_data);
                if let Ok(mut chart) = serde_json::from_value::<crate::dto::ChartDto>(chart_json) {
                    chart.r_id = Some(r_id);
                    shape.chart = Some(chart);
                }
            }
        }
    }

    if let Some(dir) = media_dir
        && let Some(rels) = rels
    {
        for rel in rels.values() {
            if rel.rel_type.contains("image")
                && let Some(target_uri) = pkg.resolve_relationship_target(uri, rel)
                && let Some(data) = pkg.get_part(&target_uri)
            {
                let filename = Path::new(&target_uri)
                    .file_name()
                    .unwrap_or(target_uri.as_ref());
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
    let background = crate::xml_parse::parse_slide_background(part_data);

    let notes = notes::resolve_notes_uri(pkg, uri).map(|notes_uri| {
        let shapes = parse_shapes(pkg, &notes_uri, media_dir).ok();
        json!({ "shapes": shapes })
    });

    let slide_layout = crate::model::parts::slide_layout_ref(pkg, uri).map(|(m, l)| {
        let name = crate::model::parts::slide_layout_uri(pkg, uri)
            .and_then(|u| crate::model::parts::c_sld_name(pkg, &u));
        json!({ "master": m, "layout": l, "name": name })
    });

    Ok(json!({
        "uri": uri,
        "shapes": shapes,
        "background": background,
        "notes": notes,
        "slide_layout": slide_layout,
    }))
}

fn master_json(pkg: &Package, master_uri: &str, media_dir: Option<&str>) -> serde_json::Value {
    let shapes = parse_shapes(pkg, master_uri, media_dir).unwrap_or_default();
    let name = crate::model::parts::c_sld_name(pkg, master_uri);
    let slide_layouts = crate::model::parts::master_slide_layout_uris(pkg, master_uri)
        .into_iter()
        .map(|layout_uri| {
            let shapes = parse_shapes(pkg, &layout_uri, media_dir).unwrap_or_default();
            let name = crate::model::parts::c_sld_name(pkg, &layout_uri);
            json!({ "uri": layout_uri, "name": name, "shapes": shapes })
        })
        .collect::<Vec<_>>();
    json!({ "uri": master_uri, "name": name, "slide_layouts": slide_layouts, "shapes": shapes })
}

/// Open a presentation and resolve its slide URIs, sharing the boilerplate
/// between `query` and `build`.
pub fn load_presentation(pkg: &Package) -> AppResult<Presentation> {
    let pres_part = pkg
        .get_part("ppt/presentation.xml")
        .ok_or_else(|| AppError::PartNotFound("ppt/presentation.xml".to_string()))?;
    let mut pres = Presentation::parse(pres_part)?;
    pres.slide_uris = pres.resolve_slide_uris(pkg);
    Ok(pres)
}

/// Build the JSON projection for a resolved path (the JSON document the
/// Markdown mirror mirrors). `media_dir` extracts referenced media when given.
fn query_value(
    pkg: &Package,
    pres: &Presentation,
    resolved: &path::ResolvedPath,
    media_dir: Option<&str>,
) -> AppResult<serde_json::Value> {
    let value = match resolved {
        path::ResolvedPath::Presentation { .. } => {
            let slides = pres
                .slide_uris
                .iter()
                .map(|uri| slide_json(pkg, uri, media_dir))
                .collect::<AppResult<Vec<_>>>()?;
            let core = pkg
                .get_part("docProps/core.xml")
                .map(core_props::parse_core_properties)
                .transpose()?
                .unwrap_or(serde_json::Value::Null);
            let slide_masters = crate::model::parts::master_uris(pkg)
                .iter()
                .map(|m| master_json(pkg, m, media_dir))
                .collect::<Vec<_>>();
            let theme = crate::model::parts::theme_uri(pkg)
                .and_then(|u| pkg.get_part(&u))
                .map(crate::model::theme::parse_theme);
            json!({
                "slide_width": pres.slide_width,
                "slide_height": pres.slide_height,
                "slides": slides,
                "slide_masters": slide_masters,
                "theme": theme,
                "core_properties": core,
            })
        }
        path::ResolvedPath::Slide { slide_idx, .. } => match slide_idx {
            None => {
                let slides = pres
                    .slide_uris
                    .iter()
                    .map(|uri| slide_json(pkg, uri, media_dir))
                    .collect::<AppResult<Vec<_>>>()?;
                json!(slides)
            }
            Some(idx) => {
                let uri = pres
                    .slide_uris
                    .get(*idx)
                    .ok_or(AppError::SlideIndexOutOfBounds(*idx))?;
                slide_json(pkg, uri, media_dir)?
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
            let shapes = parse_shapes(pkg, uri, media_dir)?;
            // Chart data is merged into the shape by `parse_shapes`.
            if let Some(path::PathSegment::Field(n)) = remaining.first()
                && n == "chart"
            {
                let idx = shape_idx
                    .ok_or_else(|| AppError::PathParse("Chart shape index required".to_string()))?;
                let chart = shapes
                    .get(idx)
                    .and_then(|s| s.get("chart").cloned())
                    .ok_or_else(|| AppError::PathParse("Shape has no chart".to_string()))?;
                json!({ "chart": chart })
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
            let theme_uri = crate::model::parts::theme_uri(pkg)
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
            let masters = crate::model::parts::master_uris(pkg);
            match master_idx {
                Some(i) => masters
                    .get(*i)
                    .ok_or(AppError::SlideIndexOutOfBounds(*i))
                    .map(|m| master_json(pkg, m, media_dir)),
                None => Ok(json!(
                    masters
                        .iter()
                        .map(|m| master_json(pkg, m, media_dir))
                        .collect::<Vec<_>>()
                )),
            }?
        }
        path::ResolvedPath::Notes {
            slide_idx: Some(idx),
            ..
        } => {
            let slide_uri = pres
                .slide_uris
                .get(*idx)
                .ok_or(AppError::SlideIndexOutOfBounds(*idx))?;
            match notes::resolve_notes_uri(pkg, slide_uri) {
                Some(notes_uri) => {
                    let shapes = parse_shapes(pkg, &notes_uri, media_dir)?;
                    json!({ "shapes": shapes })
                }
                None => serde_json::Value::Null,
            }
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
            let notes_uri = notes::resolve_notes_uri(pkg, slide_uri)
                .ok_or_else(|| AppError::PathParse("Slide has no notes slide".to_string()))?;
            let shapes = parse_shapes(pkg, &notes_uri, media_dir)?;
            match shape_idx {
                None => json!(shapes),
                Some(idx) => shapes
                    .get(*idx)
                    .ok_or(AppError::ShapeIndexOutOfBounds(*idx))?
                    .clone(),
            }
        }
    };

    Ok(value)
}

/// Project the whole presentation to the JSON document the Markdown mirror
/// mirrors.
pub fn query_document(pkg: &Package, media_dir: Option<&str>) -> AppResult<serde_json::Value> {
    let pres = load_presentation(pkg)?;
    let resolved = path::ResolvedPath::Presentation {
        remaining: Vec::new(),
    };
    query_value(pkg, &pres, &resolved, media_dir)
}
