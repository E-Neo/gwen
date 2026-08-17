use serde_json::Value;

use crate::dto::{AddShape, ShapeDto, ShapeType, ShapeTypeInput};
use crate::error::{AppError, AppResult};
use crate::model::presentation::Presentation;
use crate::opc::Package;
use crate::path::{self, PathSegment};

use super::update_diff::{Edit, EditOp};
use super::{editor, factory, update_diff, xml_edit};

const SHAPE_ATTRS: [&str; 6] = ["left", "top", "width", "height", "rotation", "name"];
const TXBODY_PROPS: [&str; 6] = [
    "word_wrap",
    "vertical_anchor",
    "margin_left",
    "margin_right",
    "margin_top",
    "margin_bottom",
];
const PARA_PROPS: [&str; 5] = [
    "alignment",
    "level",
    "line_spacing",
    "space_before",
    "space_after",
];
const READ_ONLY_SHAPE: [&str; 11] = [
    "shape_id",
    "shape_type",
    "is_placeholder",
    "has_text_frame",
    "placeholder_format",
    "image",
    "ch_off_x",
    "ch_off_y",
    "ch_ext_cx",
    "ch_ext_cy",
    "shapes",
];

/// Apply an ordered list of edits to a package, failing loudly on the first
/// error.
pub fn apply_edits(pkg: &mut Package, pres: &Presentation, edits: Vec<Edit>) -> AppResult<()> {
    for edit in update_diff::order_edits(edits) {
        apply_edit(pkg, pres, &edit)?;
    }
    Ok(())
}

fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn json_str(v: &Value) -> String {
    serde_json::to_string(v).expect("serializable JSON value")
}

/// Map a query-emitted enum value (e.g. `CENTER`, `MIDDLE`) to the STML
/// attribute spelling editors expect (e.g. `ctr`, `t`).
fn map_attr_value(prop: &str, raw: &str) -> String {
    match prop {
        "alignment" => match raw.to_uppercase().as_str() {
            "LEFT" => "l".into(),
            "CENTER" => "ctr".into(),
            "RIGHT" => "r".into(),
            "JUSTIFY" => "just".into(),
            "DISTRIBUTE" => "dist".into(),
            "THAI_DISTRIBUTE" => "thaiDist".into(),
            "JUSTIFIED_LOW" => "justLow".into(),
            _ => raw.to_string(),
        },
        "vertical_anchor" => match raw.to_uppercase().as_str() {
            "TOP" => "t".into(),
            "MIDDLE" => "ctr".into(),
            "BOTTOM" => "b".into(),
            "JUSTIFIED" => "just".into(),
            "DISTRIBUTED" => "dist".into(),
            _ => raw.to_string(),
        },
        "auto_size" => match raw.to_uppercase().as_str() {
            "NONE" => "none".into(),
            "SHAPE_TO_FIT_TEXT" => "shape_to_fit_text".into(),
            "TEXT_TO_FIT_SHAPE" => "text_to_fit_shape".into(),
            _ => raw.to_string(),
        },
        "word_wrap" => match raw {
            "true" => "square".into(),
            "false" => "none".into(),
            _ => raw.to_string(),
        },
        _ => raw.to_string(),
    }
}

/// Extract the color value from a `ColorFormatDto`-shaped JSON value.
fn color_value(v: &Value) -> String {
    v.get("rgb")
        .or_else(|| v.get("theme_color"))
        .map(scalar)
        .unwrap_or_default()
}

fn slide_uri(pres: &Presentation, idx: usize) -> AppResult<&str> {
    pres.slide_uris
        .get(idx)
        .map(|s| s.as_str())
        .ok_or(AppError::SlideIndexOutOfBounds(idx))
}

fn notes_uri(pkg: &Package, slide_uri: &str) -> AppResult<String> {
    crate::model::notes::resolve_notes_uri(pkg, slide_uri)
        .ok_or_else(|| AppError::PathParse("Slide has no notes slide".to_string()))
}

fn read_part(pkg: &Package, uri: &str) -> AppResult<Vec<u8>> {
    pkg.get_part(uri)
        .map(|p| p.to_vec())
        .ok_or_else(|| AppError::PartNotFound(uri.to_string()))
}

/// Apply a single edit to a package. Exposed so the update command can apply
/// edits one at a time and attach markdown source spans to failures.
pub fn apply_edit(pkg: &mut Package, pres: &Presentation, edit: &Edit) -> AppResult<()> {
    let resolved = path::resolve_path(&edit.path)?;
    match &resolved {
        path::ResolvedPath::Presentation { remaining } => {
            apply_presentation_edit(pkg, remaining, edit)
        }
        path::ResolvedPath::Slide {
            slide_idx: Some(i),
            remaining,
        } => {
            let uri = slide_uri(pres, *i)?.to_string();
            apply_slide_edit(pkg, &uri, remaining, edit)
        }
        path::ResolvedPath::Slide { .. } => Err(AppError::PathParse(
            "Slide index required (e.g. slides[0])".to_string(),
        )),
        path::ResolvedPath::Shape {
            slide_idx,
            shape_idx: Some(j),
            remaining,
        } => {
            let uri = slide_uri(pres, *slide_idx)?.to_string();
            apply_shape_edit(pkg, &uri, *j, remaining, edit)
        }
        path::ResolvedPath::Shape { .. } => Err(AppError::PathParse(
            "Shape index required (e.g. shapes[0])".to_string(),
        )),
        path::ResolvedPath::Notes {
            slide_idx: Some(i),
            remaining,
        } => {
            let uri = slide_uri(pres, *i)?.to_string();
            apply_notes_edit(pkg, &uri, remaining, edit)
        }
        path::ResolvedPath::Notes { .. } => Err(AppError::PathParse(
            "Slide index required (e.g. slides[0].notes)".to_string(),
        )),
        path::ResolvedPath::NotesShape {
            slide_idx,
            shape_idx: Some(j),
            remaining,
        } => {
            let slide = slide_uri(pres, *slide_idx)?.to_string();
            let uri = notes_uri(pkg, &slide)?;
            apply_shape_edit(pkg, &uri, *j, remaining, edit)
        }
        path::ResolvedPath::NotesShape { .. } => Err(AppError::PathParse(
            "Shape index required (e.g. notes.shapes[0])".to_string(),
        )),
        path::ResolvedPath::Theme { remaining } => apply_theme_edit(pkg, remaining, edit),
        path::ResolvedPath::Master {
            master_idx: Some(i),
            remaining,
        } => {
            let masters = crate::model::parts::master_uris(pkg);
            let master_uri = masters
                .get(*i)
                .ok_or(AppError::SlideIndexOutOfBounds(*i))?
                .clone();
            apply_master_edit(pkg, &master_uri, remaining, edit)
        }
        path::ResolvedPath::Master { .. } => Err(AppError::PathParse(
            "Master index required (e.g. slide_masters[0])".to_string(),
        )),
    }
}

fn apply_presentation_edit(
    pkg: &mut Package,
    remaining: &[PathSegment],
    edit: &Edit,
) -> AppResult<()> {
    match remaining {
        [PathSegment::Field(n)] if n == "slide_width" || n == "slide_height" => {
            let value = edit
                .value
                .as_ref()
                .ok_or_else(|| AppError::InvalidValue(format!("{n} cannot be deleted")))?;
            let uri = "ppt/presentation.xml".to_string();
            let data = read_part(pkg, &uri)?;
            let new = editor::replace_presentation_property(&data, remaining, &scalar(value))?;
            pkg.set_part(&uri, new);
            Ok(())
        }
        [PathSegment::Field(n), PathSegment::Field(prop)] if n == "core_properties" => {
            let uri = "docProps/core.xml".to_string();
            let data = read_part(pkg, &uri)?;
            match edit.op {
                EditOp::Set => {
                    let value = edit.value.as_ref().ok_or(AppError::InvalidValue(
                        "core property value required".to_string(),
                    ))?;
                    let new = xml_edit::replace_core_property(&data, prop, &scalar(value))?;
                    pkg.set_part(&uri, new);
                }
                EditOp::Delete => {
                    let new = xml_edit::delete_core_property(&data, prop)?;
                    pkg.set_part(&uri, new);
                }
                EditOp::Insert => {
                    return Err(AppError::PathParse(
                        "core_properties takes a property name".to_string(),
                    ));
                }
            }
            Ok(())
        }
        [PathSegment::Field(n), PathSegment::Field(prop), ..] if n == "core_properties" => {
            Err(AppError::PathParse(format!(
                "core_properties.{prop} is a leaf; nothing may follow it"
            )))
        }
        _ => Err(AppError::PathParse(format!(
            "Unsupported presentation path: {}",
            path_str(&edit.path)
        ))),
    }
}

fn apply_theme_edit(pkg: &mut Package, remaining: &[PathSegment], edit: &Edit) -> AppResult<()> {
    if edit.op != EditOp::Set {
        return Err(AppError::PathParse(
            "Theme colors and fonts can only be set".to_string(),
        ));
    }
    let value = edit
        .value
        .as_ref()
        .ok_or(AppError::InvalidValue("theme value required".to_string()))?;
    let theme_uri = crate::model::parts::theme_uri(pkg)
        .ok_or_else(|| AppError::PathParse("Presentation has no theme".to_string()))?;
    let data = read_part(pkg, &theme_uri)?;
    let new = xml_edit::replace_theme_property(&data, remaining, &scalar(value))?;
    pkg.set_part(&theme_uri, new);
    Ok(())
}

fn apply_slide_edit(
    pkg: &mut Package,
    uri: &str,
    remaining: &[PathSegment],
    edit: &Edit,
) -> AppResult<()> {
    match remaining {
        [PathSegment::Field(n), PathSegment::Index(j), tail @ ..] if n == "shapes" => {
            apply_shape_edit(pkg, uri, *j, tail, edit)
        }
        [PathSegment::Field(n), tail @ ..] if n == "background" => {
            if edit.op == EditOp::Set
                && tail
                    == [
                        PathSegment::Field("fill".to_string()),
                        PathSegment::Field("color".to_string()),
                    ]
            {
                let value = edit.value.as_ref().ok_or(AppError::InvalidValue(
                    "background color required".to_string(),
                ))?;
                let data = read_part(pkg, uri)?;
                let new = xml_edit::set_slide_background(&data, &scalar(value))?;
                pkg.set_part(uri, new);
                Ok(())
            } else if edit.op == EditOp::Set
                && tail
                    == [
                        PathSegment::Field("fill".to_string()),
                        PathSegment::Field("type".to_string()),
                    ]
            {
                // The fill kind is marker metadata from the `background=SOLID:…`
                // comment; the color edit performs the XML write.
                Ok(())
            } else {
                Err(AppError::PathParse(
                    "Only background.fill.color is supported".to_string(),
                ))
            }
        }
        [PathSegment::Field(n), tail @ ..] if n == "notes" => {
            apply_notes_edit(pkg, uri, tail, edit)
        }
        [PathSegment::Field(n)] if n == "title" => match edit.op {
            EditOp::Set => {
                let value = edit
                    .value
                    .as_ref()
                    .ok_or(AppError::InvalidValue("title value required".to_string()))?;
                let data = read_part(pkg, uri)?;
                let new = xml_edit::replace_slide_title(&data, &scalar(value))?;
                pkg.set_part(uri, new);
                Ok(())
            }
            _ => Err(AppError::PathParse(
                "slides[N].title can only be set".to_string(),
            )),
        },
        [PathSegment::Field(n), ..] if n == "slide_layout" => Err(AppError::PathParse(
            "slide_layout is a read-only reference; edit the layout via slide_masters".to_string(),
        )),
        _ => Err(AppError::PathParse(format!(
            "Unsupported slide path: {}",
            path_str(&edit.path)
        ))),
    }
}

fn apply_notes_edit(
    pkg: &mut Package,
    slide_uri: &str,
    remaining: &[PathSegment],
    edit: &Edit,
) -> AppResult<()> {
    match remaining {
        [PathSegment::Field(n), PathSegment::Index(j), tail @ ..] if n == "shapes" => {
            let uri = notes_uri(pkg, slide_uri)?;
            apply_shape_edit(pkg, &uri, *j, tail, edit)
        }
        [] => match edit.op {
            EditOp::Delete => {
                delete_notes_slide(pkg, slide_uri)?;
                Ok(())
            }
            EditOp::Set => {
                let value = edit
                    .value
                    .as_ref()
                    .ok_or(AppError::InvalidValue("notes value required".to_string()))?;
                if notes_uri(pkg, slide_uri).is_ok() {
                    return Err(AppError::PathParse(
                        "Slide already has notes; edit slides[N].notes.shapes[M] instead"
                            .to_string(),
                    ));
                }
                create_notes_slide(pkg, slide_uri, value)
            }
            EditOp::Insert => Err(AppError::PathParse("notes is not an array".to_string())),
        },
        _ => Err(AppError::PathParse(format!(
            "Unsupported notes path: {}",
            path_str(&edit.path)
        ))),
    }
}

fn apply_master_edit(
    pkg: &mut Package,
    master_uri: &str,
    remaining: &[PathSegment],
    edit: &Edit,
) -> AppResult<()> {
    match remaining {
        [
            PathSegment::Field(n),
            PathSegment::Index(l),
            PathSegment::Field(s),
            PathSegment::Index(j),
            tail @ ..,
        ] if n == "slide_layouts" && s == "shapes" => {
            let layouts = crate::model::parts::master_slide_layout_uris(pkg, master_uri);
            let layout_uri = layouts
                .get(*l)
                .ok_or(AppError::SlideIndexOutOfBounds(*l))?
                .clone();
            apply_shape_edit(pkg, &layout_uri, *j, tail, edit)
        }
        [PathSegment::Field(n), PathSegment::Index(j), tail @ ..] if n == "shapes" => {
            apply_shape_edit(pkg, master_uri, *j, tail, edit)
        }
        [PathSegment::Field(n), ..] if n == "name" => Err(AppError::PathParse(
            "master/layout name is read-only".to_string(),
        )),
        _ => Err(AppError::PathParse(format!(
            "Unsupported master path: {}",
            path_str(&edit.path)
        ))),
    }
}

fn path_str(segments: &[PathSegment]) -> String {
    let mut out = String::new();
    for (i, seg) in segments.iter().enumerate() {
        if i == 0 && matches!(seg, PathSegment::Field(f) if f == "p") {
            continue;
        }
        match seg {
            PathSegment::Field(f) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(f);
            }
            PathSegment::Index(k) => out.push_str(&format!("[{k}]")),
        }
    }
    out
}

/// Apply an edit to a shape's properties within a part (a slide, notes slide,
/// master, or layout). `remaining` is the path after `shapes[N]`.
fn apply_shape_edit(
    pkg: &mut Package,
    uri: &str,
    shape_idx: usize,
    remaining: &[PathSegment],
    edit: &Edit,
) -> AppResult<()> {
    // Chart data lives in its own OPC part.
    if matches!(remaining, [PathSegment::Field(n), ..] if n == "chart") {
        let chart_uri = crate::model::parts::resolve_chart_part(pkg, uri, shape_idx)?;
        let data = read_part(pkg, &chart_uri)?;
        let new = shape_chart_edit(&data, remaining, edit)?;
        pkg.set_part(&chart_uri, new);
        return Ok(());
    }

    let data = read_part(pkg, uri)?;
    let new = shape_part_edit(&data, shape_idx, remaining, edit)?;
    pkg.set_part(uri, new);
    Ok(())
}

/// Edits that mutate the slide part holding the shape.
fn shape_part_edit(
    xml: &[u8],
    shape_idx: usize,
    remaining: &[PathSegment],
    edit: &Edit,
) -> AppResult<Vec<u8>> {
    let op = edit.op;
    let value = edit.value.as_ref();
    let seg = remaining.first();

    if let Some(PathSegment::Field(name)) = seg
        && READ_ONLY_SHAPE.contains(&name.as_str())
    {
        return Err(AppError::PathParse(format!(
            "Shape field '{name}' is read-only and cannot be edited"
        )));
    }

    match (op, seg) {
        (EditOp::Delete, None) => editor::remove_shape(xml, shape_idx),
        (EditOp::Insert, None) => add_shape_to_part(
            xml,
            shape_idx,
            value.ok_or_else(|| AppError::InvalidValue("shape value required".to_string()))?,
        ),

        (EditOp::Set, Some(PathSegment::Field(n))) if SHAPE_ATTRS.contains(&n.as_str()) => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            editor::replace_shape_attr(xml, shape_idx, remaining, &scalar(val))
        }
        (EditOp::Delete, Some(PathSegment::Field(n))) if SHAPE_ATTRS.contains(&n.as_str()) => Err(
            AppError::PathParse(format!("Shape attribute '{n}' cannot be deleted")),
        ),

        (EditOp::Set, Some(PathSegment::Field(n))) if n == "text" => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            xml_edit::replace_shape_text_lossless(xml, shape_idx, &scalar(val))
        }
        (EditOp::Delete, Some(PathSegment::Field(n))) if n == "text" => Err(AppError::PathParse(
            "text is derived from text_frame; edit runs instead".to_string(),
        )),

        (EditOp::Set, Some(PathSegment::Field(n))) if n == "auto_shape_type" => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            xml_edit::set_auto_shape_type_lossless(xml, shape_idx, &scalar(val))
        }

        (EditOp::Set, Some(PathSegment::Field(n))) if n == "fill" => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            xml_edit::replace_shape_fill_lossless(xml, shape_idx, remaining, &json_str(val))
        }
        (EditOp::Delete, Some(PathSegment::Field(n))) if n == "fill" => {
            xml_edit::delete_shape_fill(xml, shape_idx)
        }

        (EditOp::Set, Some(PathSegment::Field(n))) if n == "outline" => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            xml_edit::replace_shape_outline_lossless(xml, shape_idx, remaining, &json_str(val))
        }
        (EditOp::Delete, Some(PathSegment::Field(n))) if n == "outline" => {
            xml_edit::delete_shape_outline(xml, shape_idx)
        }

        (EditOp::Set, Some(PathSegment::Field(n))) if n == "crop" => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            match &remaining[1..] {
                [] => set_whole_crop(xml, shape_idx, val),
                [PathSegment::Field(_)] => {
                    xml_edit::replace_picture_crop(xml, shape_idx, remaining, &scalar(val))
                }
                _ => Err(AppError::PathParse(
                    "crop takes a side (left/top/right/bottom)".to_string(),
                )),
            }
        }
        (EditOp::Delete, Some(PathSegment::Field(n))) if n == "crop" => match &remaining[1..] {
            [] => xml_edit::delete_picture_crop(xml, shape_idx),
            [PathSegment::Field(side)] => xml_edit::delete_picture_crop_side(xml, shape_idx, side),
            _ => Err(AppError::PathParse(
                "crop takes a side (left/top/right/bottom)".to_string(),
            )),
        },

        (EditOp::Set, Some(PathSegment::Field(n))) if n == "text_frame" => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            text_frame_set(xml, shape_idx, &remaining[1..], val)
        }
        (EditOp::Delete, Some(PathSegment::Field(n))) if n == "text_frame" => {
            if remaining.len() == 1 {
                xml_edit::delete_shape_text_frame(xml, shape_idx)
            } else {
                text_frame_delete(xml, shape_idx, &remaining[1..])
            }
        }
        (EditOp::Insert, Some(PathSegment::Field(n))) if n == "text_frame" => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            text_frame_insert(xml, shape_idx, &remaining[1..], val)
        }

        (EditOp::Set, Some(PathSegment::Field(n))) if n == "table" => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            table_set(xml, shape_idx, &remaining[1..], val)
        }
        (EditOp::Delete, Some(PathSegment::Field(n))) if n == "table" => {
            table_delete(xml, shape_idx, &remaining[1..])
        }
        (EditOp::Insert, Some(PathSegment::Field(n))) if n == "table" => {
            let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
            table_insert(xml, shape_idx, &remaining[1..], val)
        }

        (EditOp::Set, Some(PathSegment::Field(n))) if n == "shapes" => Err(AppError::PathParse(
            "Group child shapes are not editable via update".to_string(),
        )),

        _ => Err(AppError::PathParse(format!(
            "Unsupported shape path: shapes[{shape_idx}].{}",
            path_str(remaining)
        ))),
    }
}

/// Set a whole crop from a `CropDto`-shaped JSON value.
fn set_whole_crop(xml: &[u8], shape_idx: usize, value: &Value) -> AppResult<Vec<u8>> {
    let mut out = xml.to_vec();
    for side in ["left", "top", "right", "bottom"] {
        if let Some(v) = value.get(side) {
            let segments = [
                PathSegment::Field("crop".to_string()),
                PathSegment::Field(side.to_string()),
            ];
            out = xml_edit::replace_picture_crop(&out, shape_idx, &segments, &scalar(v))?;
        }
    }
    Ok(out)
}

// -- text_frame ------------------------------------------------------------

fn text_frame_set(
    xml: &[u8],
    shape_idx: usize,
    rest: &[PathSegment],
    value: &Value,
) -> AppResult<Vec<u8>> {
    match rest {
        [] => xml_edit::replace_or_create_text_frame_lossless(xml, shape_idx, &json_str(value)),
        [PathSegment::Field(n)] if n == "default_paragraph_style" => {
            xml_edit::replace_lst_style_lossless(xml, shape_idx, &json_str(value))
        }
        [PathSegment::Field(n), tail @ ..] if n == "default_paragraph_style" => Err(
            AppError::PathParse("default_paragraph_style is edited as a whole".to_string()),
        ),
        [PathSegment::Field(n), tail @ ..] if n == "paragraphs" => {
            paragraph_set(xml, shape_idx, tail, value)
        }
        [PathSegment::Field(n)] if TXBODY_PROPS.contains(&n.as_str()) => {
            let mapped = map_attr_value(n, &scalar(value));
            let path = [
                PathSegment::Field("text_frame".to_string()),
                PathSegment::Field(n.clone()),
            ];
            xml_edit::replace_shape_property_lossless(xml, shape_idx, &path, &mapped)
        }
        [PathSegment::Field(n)] if n == "auto_size" => {
            let mapped = map_attr_value("auto_size", &scalar(value));
            let path = [
                PathSegment::Field("text_frame".to_string()),
                PathSegment::Field("auto_size".to_string()),
            ];
            xml_edit::replace_shape_property_lossless(xml, shape_idx, &path, &mapped)
        }
        _ => Err(AppError::PathParse(format!(
            "Unsupported text_frame path: text_frame.{}",
            path_str(rest)
        ))),
    }
}

fn text_frame_delete(xml: &[u8], shape_idx: usize, rest: &[PathSegment]) -> AppResult<Vec<u8>> {
    match rest {
        [PathSegment::Field(n)] if TXBODY_PROPS.contains(&n.as_str()) => {
            xml_edit::delete_txbody_prop(xml, shape_idx, n)
        }
        [PathSegment::Field(n)] if n == "auto_size" => {
            xml_edit::delete_txbody_prop(xml, shape_idx, "auto_size")
        }
        [PathSegment::Field(n), tail @ ..] if n == "paragraphs" => {
            paragraph_delete(xml, shape_idx, tail)
        }
        _ => Err(AppError::PathParse(format!(
            "Cannot delete text_frame.{}",
            path_str(rest)
        ))),
    }
}

fn text_frame_insert(
    xml: &[u8],
    shape_idx: usize,
    rest: &[PathSegment],
    value: &Value,
) -> AppResult<Vec<u8>> {
    match rest {
        [PathSegment::Field(n), PathSegment::Index(k)] if n == "paragraphs" => {
            let path = [
                PathSegment::Field("text_frame".to_string()),
                PathSegment::Field("paragraphs".to_string()),
                PathSegment::Index(*k),
            ];
            editor::add_to_text_frame(xml, shape_idx, &path, &json_str(value))
        }
        [
            PathSegment::Field(n),
            PathSegment::Index(k),
            PathSegment::Field(r),
            PathSegment::Index(m),
        ] if n == "paragraphs" && r == "runs" => {
            let path = [
                PathSegment::Field("text_frame".to_string()),
                PathSegment::Field("paragraphs".to_string()),
                PathSegment::Index(*k),
                PathSegment::Field("runs".to_string()),
                PathSegment::Index(*m),
            ];
            editor::add_to_text_frame(xml, shape_idx, &path, &json_str(value))
        }
        _ => Err(AppError::PathParse(format!(
            "Unsupported text_frame insert: text_frame.{}",
            path_str(rest)
        ))),
    }
}

fn paragraph_set(
    xml: &[u8],
    shape_idx: usize,
    rest: &[PathSegment],
    value: &Value,
) -> AppResult<Vec<u8>> {
    let [PathSegment::Index(k), tail @ ..] = rest else {
        return Err(AppError::PathParse("Paragraph index required".to_string()));
    };
    match tail {
        [] => xml_edit::replace_paragraph_lossless(xml, shape_idx, *k, &json_str(value)),
        [PathSegment::Field(n), rtail @ ..] if n == "runs" => {
            run_set(xml, shape_idx, *k, rtail, value)
        }
        [PathSegment::Field(n)] if PARA_PROPS.contains(&n.as_str()) => {
            let mapped = map_attr_value(n, &scalar(value));
            let path = [
                PathSegment::Field("text_frame".to_string()),
                PathSegment::Field("paragraphs".to_string()),
                PathSegment::Index(*k),
                PathSegment::Field(n.clone()),
            ];
            xml_edit::replace_shape_property_lossless(xml, shape_idx, &path, &mapped)
        }
        [PathSegment::Field(n)] if n == "font" => {
            xml_edit::replace_para_font_lossless(xml, shape_idx, *k, &json_str(value))
        }
        [PathSegment::Field(n), ftail @ ..] if n == "font" => {
            para_font_set(xml, shape_idx, *k, ftail, value)
        }
        [PathSegment::Field(n)] if n == "default_paragraph_style" => Err(AppError::PathParse(
            "default_paragraph_style lives on text_frame, not a paragraph".to_string(),
        )),
        _ => Err(AppError::PathParse(format!(
            "Unsupported paragraph path: paragraphs[{k}].{}",
            path_str(tail)
        ))),
    }
}

fn paragraph_delete(xml: &[u8], shape_idx: usize, rest: &[PathSegment]) -> AppResult<Vec<u8>> {
    match rest {
        [PathSegment::Index(k)] => {
            let path = [
                PathSegment::Field("text_frame".to_string()),
                PathSegment::Field("paragraphs".to_string()),
                PathSegment::Index(*k),
            ];
            editor::remove_from_text_frame(xml, shape_idx, &path)
        }
        [
            PathSegment::Index(k),
            PathSegment::Field(n),
            PathSegment::Index(m),
        ] if n == "runs" => {
            let path = [
                PathSegment::Field("text_frame".to_string()),
                PathSegment::Field("paragraphs".to_string()),
                PathSegment::Index(*k),
                PathSegment::Field("runs".to_string()),
                PathSegment::Index(*m),
            ];
            editor::remove_from_text_frame(xml, shape_idx, &path)
        }
        [
            PathSegment::Index(k),
            PathSegment::Field(n),
            PathSegment::Index(m),
            PathSegment::Field(f),
        ] if n == "runs" && f == "font" => {
            xml_edit::delete_run_font_lossless(xml, shape_idx, *k, *m)
        }
        [PathSegment::Index(k), PathSegment::Field(n)] if PARA_PROPS.contains(&n.as_str()) => {
            xml_edit::delete_para_prop(xml, shape_idx, *k, n)
        }
        [PathSegment::Index(k), PathSegment::Field(n)] if n == "font" => {
            xml_edit::delete_para_font_lossless(xml, shape_idx, *k)
        }
        _ => Err(AppError::PathParse(format!(
            "Cannot delete paragraphs.{}",
            path_str(rest)
        ))),
    }
}

fn run_set(
    xml: &[u8],
    shape_idx: usize,
    para_idx: usize,
    rest: &[PathSegment],
    value: &Value,
) -> AppResult<Vec<u8>> {
    let [PathSegment::Index(m), tail @ ..] = rest else {
        // Whole-array replacement: a paragraph that gained runs (e.g. an empty
        // title paragraph that now carries heading text) arrives as a Set on
        // the `runs` field with no index.
        return xml_edit::replace_paragraph_runs_lossless(
            xml,
            shape_idx,
            para_idx,
            &json_str(value),
        );
    };
    match tail {
        [] => xml_edit::replace_run_lossless(xml, shape_idx, para_idx, *m, &json_str(value)),
        [PathSegment::Field(n)] if n == "text" => {
            let path = [
                PathSegment::Field("text_frame".to_string()),
                PathSegment::Field("paragraphs".to_string()),
                PathSegment::Index(para_idx),
                PathSegment::Field("runs".to_string()),
                PathSegment::Index(*m),
                PathSegment::Field("text".to_string()),
            ];
            xml_edit::replace_shape_property_lossless(xml, shape_idx, &path, &scalar(value))
        }
        [PathSegment::Field(n)] if n == "font" => {
            xml_edit::replace_run_font_lossless(xml, shape_idx, para_idx, *m, &json_str(value))
        }
        [PathSegment::Field(n), ftail @ ..] if n == "font" => {
            run_font_set(xml, shape_idx, para_idx, *m, ftail, value)
        }
        _ => Err(AppError::PathParse(format!(
            "Unsupported run path: runs[{m}].{}",
            path_str(tail)
        ))),
    }
}

fn run_font_set(
    xml: &[u8],
    shape_idx: usize,
    para_idx: usize,
    run_idx: usize,
    rest: &[PathSegment],
    value: &Value,
) -> AppResult<Vec<u8>> {
    let [PathSegment::Field(prop)] = rest else {
        return Err(AppError::PathParse("Font property required".to_string()));
    };
    if prop == "hyperlink" {
        return Err(AppError::PathParse("hyperlink is read-only".to_string()));
    }
    let mut path = vec![
        PathSegment::Field("text_frame".to_string()),
        PathSegment::Field("paragraphs".to_string()),
        PathSegment::Index(para_idx),
        PathSegment::Field("runs".to_string()),
        PathSegment::Index(run_idx),
        PathSegment::Field("font".to_string()),
    ];
    let val = if prop == "color" && value.is_object() {
        color_value(value)
    } else {
        scalar(value)
    };
    path.push(PathSegment::Field(prop.clone()));
    xml_edit::replace_shape_property_lossless(xml, shape_idx, &path, &val)
}

fn para_font_set(
    xml: &[u8],
    shape_idx: usize,
    para_idx: usize,
    rest: &[PathSegment],
    value: &Value,
) -> AppResult<Vec<u8>> {
    let [PathSegment::Field(prop)] = rest else {
        return Err(AppError::PathParse("Font property required".to_string()));
    };
    let mut path = vec![
        PathSegment::Field("text_frame".to_string()),
        PathSegment::Field("paragraphs".to_string()),
        PathSegment::Index(para_idx),
        PathSegment::Field("font".to_string()),
    ];
    let val = if prop == "color" && value.is_object() {
        color_value(value)
    } else {
        scalar(value)
    };
    path.push(PathSegment::Field(prop.clone()));
    xml_edit::replace_shape_property_lossless(xml, shape_idx, &path, &val)
}

// -- table -----------------------------------------------------------------

fn table_set(
    xml: &[u8],
    shape_idx: usize,
    rest: &[PathSegment],
    value: &Value,
) -> AppResult<Vec<u8>> {
    match rest {
        [] => xml_edit::replace_whole_table_lossless(xml, shape_idx, &json_str(value)),
        [PathSegment::Field(n), PathSegment::Index(r), tail @ ..] if n == "rows" => match tail {
            [] => xml_edit::replace_table_row_lossless(xml, shape_idx, *r, &json_str(value)),
            [PathSegment::Field(c), PathSegment::Index(ci), ctail @ ..] if c == "cells" => {
                match ctail {
                    [] => xml_edit::replace_table_cell_lossless(
                        xml,
                        shape_idx,
                        *r,
                        *ci,
                        &json_str(value),
                    ),
                    [PathSegment::Field(t), ttail @ ..] if t == "text_frame" => {
                        table_cell_text(xml, shape_idx, *r, *ci, ttail, value)
                    }
                    _ => Err(AppError::PathParse(
                        "Unsupported table cell path".to_string(),
                    )),
                }
            }
            _ => Err(AppError::PathParse(format!(
                "Unsupported table row path: rows[{r}].{}",
                path_str(tail)
            ))),
        },
        [PathSegment::Field(n), PathSegment::Index(c)] if n == "grid" => {
            xml_edit::replace_table_grid_col_lossless(xml, shape_idx, *c, &json_str(value))
        }
        _ => Err(AppError::PathParse(format!(
            "Unsupported table path: table.{}",
            path_str(rest)
        ))),
    }
}

fn table_cell_text(
    xml: &[u8],
    shape_idx: usize,
    row_idx: usize,
    cell_idx: usize,
    rest: &[PathSegment],
    value: &Value,
) -> AppResult<Vec<u8>> {
    let mut path = vec![
        PathSegment::Field("table".to_string()),
        PathSegment::Field("rows".to_string()),
        PathSegment::Index(row_idx),
        PathSegment::Field("cells".to_string()),
        PathSegment::Index(cell_idx),
    ];
    path.push(PathSegment::Field("text_frame".to_string()));
    match rest {
        [] => {
            xml_edit::replace_table_cell_property_lossless(xml, shape_idx, &path, &json_str(value))
        }
        [PathSegment::Field(n), tail @ ..] if n == "paragraphs" => {
            path.push(PathSegment::Field("paragraphs".to_string()));
            path.extend(tail.iter().cloned());
            xml_edit::replace_table_cell_property_lossless(xml, shape_idx, &path, &scalar(value))
        }
        [PathSegment::Field(n)] if TXBODY_PROPS.contains(&n.as_str()) => {
            let mapped = map_attr_value(n, &scalar(value));
            path.push(PathSegment::Field(n.clone()));
            xml_edit::replace_table_cell_property_lossless(xml, shape_idx, &path, &mapped)
        }
        _ => Err(AppError::PathParse(
            "Unsupported table cell text_frame path".to_string(),
        )),
    }
}

fn table_delete(xml: &[u8], shape_idx: usize, rest: &[PathSegment]) -> AppResult<Vec<u8>> {
    match rest {
        [PathSegment::Field(n), PathSegment::Index(r)] if n == "rows" => {
            let path = [
                PathSegment::Field("table".to_string()),
                PathSegment::Field("rows".to_string()),
                PathSegment::Index(*r),
            ];
            xml_edit::remove_table_row_lossless(xml, shape_idx, &path)
        }
        [PathSegment::Field(n), PathSegment::Index(c)] if n == "grid" => {
            let path = [
                PathSegment::Field("table".to_string()),
                PathSegment::Field("grid".to_string()),
                PathSegment::Index(*c),
            ];
            xml_edit::remove_table_column_lossless(xml, shape_idx, &path)
        }
        [
            PathSegment::Field(n),
            PathSegment::Index(r),
            PathSegment::Field(c),
            PathSegment::Index(ci),
        ] if n == "rows" && c == "cells" => {
            let path = [
                PathSegment::Field("table".to_string()),
                PathSegment::Field("rows".to_string()),
                PathSegment::Index(*r),
                PathSegment::Field("cells".to_string()),
                PathSegment::Index(*ci),
            ];
            editor::remove_from_table(xml, shape_idx, &path)
        }
        [
            PathSegment::Field(n),
            PathSegment::Index(r),
            PathSegment::Field(c),
            PathSegment::Index(ci),
            PathSegment::Field(t),
        ] if n == "rows" && c == "cells" && t == "text_frame" => {
            let path = [
                PathSegment::Field("table".to_string()),
                PathSegment::Field("rows".to_string()),
                PathSegment::Index(*r),
                PathSegment::Field("cells".to_string()),
                PathSegment::Index(*ci),
                PathSegment::Field("text_frame".to_string()),
            ];
            xml_edit::replace_table_cell_property_lossless(
                xml,
                shape_idx,
                &path,
                "{\"paragraphs\":[{\"level\":0}]}",
            )
        }
        [
            PathSegment::Field(n),
            PathSegment::Index(r),
            PathSegment::Field(c),
            PathSegment::Index(ci),
            PathSegment::Field(t),
            PathSegment::Field(p),
            PathSegment::Index(pi),
            PathSegment::Field(runs),
        ] if n == "rows" && c == "cells" && t == "text_frame" && p == "paragraphs" => {
            let path = [
                PathSegment::Field("table".to_string()),
                PathSegment::Field("rows".to_string()),
                PathSegment::Index(*r),
                PathSegment::Field("cells".to_string()),
                PathSegment::Index(*ci),
                PathSegment::Field("text_frame".to_string()),
                PathSegment::Field("paragraphs".to_string()),
                PathSegment::Index(*pi),
                PathSegment::Field("runs".to_string()),
            ];
            xml_edit::replace_table_cell_property_lossless(xml, shape_idx, &path, "[]")
        }
        _ => Err(AppError::PathParse(format!(
            "Cannot delete table.{}",
            path_str(rest)
        ))),
    }
}

fn table_insert(
    xml: &[u8],
    shape_idx: usize,
    rest: &[PathSegment],
    value: &Value,
) -> AppResult<Vec<u8>> {
    match rest {
        [PathSegment::Field(n), PathSegment::Index(i)] if n == "rows" => {
            let path = [
                PathSegment::Field("table".to_string()),
                PathSegment::Field("rows".to_string()),
                PathSegment::Index(*i),
            ];
            xml_edit::add_table_row_lossless(xml, shape_idx, &path, &json_str(value))
        }
        [PathSegment::Field(n), PathSegment::Index(i)] if n == "grid" => {
            let path = [
                PathSegment::Field("table".to_string()),
                PathSegment::Field("grid".to_string()),
                PathSegment::Index(*i),
            ];
            xml_edit::add_table_column_lossless(xml, shape_idx, &path, &json_str(value))
        }
        [
            PathSegment::Field(n),
            PathSegment::Index(r),
            PathSegment::Field(c),
            PathSegment::Index(ci),
        ] if n == "rows" && c == "cells" => {
            let path = [
                PathSegment::Field("table".to_string()),
                PathSegment::Field("rows".to_string()),
                PathSegment::Index(*r),
                PathSegment::Field("cells".to_string()),
                PathSegment::Index(*ci),
            ];
            editor::add_to_table(xml, shape_idx, &path, &json_str(value))
        }
        _ => Err(AppError::PathParse(format!(
            "Unsupported table insert: table.{}",
            path_str(rest)
        ))),
    }
}

// -- chart -----------------------------------------------------------------

fn shape_chart_edit(xml: &[u8], remaining: &[PathSegment], edit: &Edit) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    let value = edit.value.as_ref();
    match (edit.op, inner) {
        (EditOp::Set, [PathSegment::Field(n), ..]) if n == "chart_type" || n == "r_id" => Err(
            AppError::PathParse(format!("Chart field '{n}' is read-only")),
        ),
        (EditOp::Set, [PathSegment::Field(n), tail @ ..]) if n == "series" => {
            chart_series_set(xml, tail, value)
        }
        (EditOp::Delete, [PathSegment::Field(n), tail @ ..]) if n == "series" => {
            chart_series_delete(xml, tail)
        }
        (EditOp::Insert, [PathSegment::Field(n), tail @ ..]) if n == "series" => {
            chart_series_insert(xml, tail, value)
        }
        (EditOp::Set, []) | (EditOp::Delete, []) => Err(AppError::PathParse(
            "Whole-chart replacement is not supported; edit chart.series instead".to_string(),
        )),
        _ => Err(AppError::PathParse(format!(
            "Unsupported chart path: chart.{}",
            path_str(inner)
        ))),
    }
}

fn chart_series_set(xml: &[u8], rest: &[PathSegment], value: Option<&Value>) -> AppResult<Vec<u8>> {
    let [PathSegment::Index(k), tail @ ..] = rest else {
        return Err(AppError::PathParse("Series index required".to_string()));
    };
    let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
    match tail {
        [] => xml_edit::replace_chart_series_lossless(
            xml,
            &[
                PathSegment::Field("chart".into()),
                PathSegment::Field("series".into()),
                PathSegment::Index(*k),
            ],
            &json_str(val),
        ),
        [PathSegment::Field(n)] if n == "name" => {
            let path = [
                PathSegment::Field("chart".to_string()),
                PathSegment::Field("series".to_string()),
                PathSegment::Index(*k),
                PathSegment::Field("name".to_string()),
            ];
            xml_edit::replace_chart_property_lossless(xml, &path, &scalar(val))
        }
        [PathSegment::Field(n), PathSegment::Index(j)] if n == "categories" || n == "values" => {
            let mut path = vec![
                PathSegment::Field("chart".to_string()),
                PathSegment::Field("series".to_string()),
                PathSegment::Index(*k),
                PathSegment::Field(n.clone()),
                PathSegment::Index(*j),
            ];
            let raw = if n == "values" && val.is_number() {
                val.to_string()
            } else {
                scalar(val)
            };
            path.pop();
            path.push(PathSegment::Index(*j));
            xml_edit::replace_chart_property_lossless(xml, &path, &raw)
        }
        _ => Err(AppError::PathParse(format!(
            "Unsupported chart series path: series[{k}].{}",
            path_str(tail)
        ))),
    }
}

fn chart_series_delete(xml: &[u8], rest: &[PathSegment]) -> AppResult<Vec<u8>> {
    match rest {
        [PathSegment::Index(k)] => {
            let path = [
                PathSegment::Field("chart".to_string()),
                PathSegment::Field("series".to_string()),
                PathSegment::Index(*k),
            ];
            xml_edit::remove_chart_series_lossless(xml, &path)
        }
        [
            PathSegment::Index(k),
            PathSegment::Field(n),
            PathSegment::Index(j),
        ] if n == "categories" || n == "values" => {
            let path = [
                PathSegment::Field("chart".to_string()),
                PathSegment::Field("series".to_string()),
                PathSegment::Index(*k),
                PathSegment::Field(n.clone()),
                PathSegment::Index(*j),
            ];
            xml_edit::remove_chart_point_lossless(xml, &path)
        }
        _ => Err(AppError::PathParse(
            "Cannot delete that chart element".to_string(),
        )),
    }
}

fn chart_series_insert(
    xml: &[u8],
    rest: &[PathSegment],
    value: Option<&Value>,
) -> AppResult<Vec<u8>> {
    let val = value.ok_or_else(|| AppError::InvalidValue("value required".to_string()))?;
    match rest {
        [PathSegment::Index(_)] => {
            // Appending a whole series (chart.series[N]).
            let path = [
                PathSegment::Field("chart".to_string()),
                PathSegment::Field("series".to_string()),
            ];
            xml_edit::add_chart_series_lossless(xml, &path, &json_str(val))
        }
        [
            PathSegment::Index(k),
            PathSegment::Field(n),
            PathSegment::Index(j),
        ] if n == "categories" || n == "values" => {
            // Appending a category/value point.
            let path = vec![
                PathSegment::Field("chart".to_string()),
                PathSegment::Field("series".to_string()),
                PathSegment::Index(*k),
                PathSegment::Field(n.clone()),
            ];
            xml_edit::add_chart_point_lossless(xml, &path, *j, &scalar(val))
        }
        _ => Err(AppError::PathParse("Unsupported chart insert".to_string())),
    }
}

// -- shape insert (appending a new shape to shapes[]) ----------------------

fn shape_dto_to_add(dto: &ShapeDto) -> AppResult<AddShape> {
    let shape_type = match dto.shape_type {
        ShapeType::TextBox => ShapeTypeInput::Textbox,
        ShapeType::AutoShape => ShapeTypeInput::AutoShape,
        ShapeType::Table => ShapeTypeInput::Table,
        _ => {
            return Err(AppError::InvalidValue(format!(
                "Adding a {0:?} shape is not supported; only textbox, autoshape and table",
                dto.shape_type
            )));
        }
    };
    let text = dto.text_frame.as_ref().map(|tf| {
        tf.paragraphs
            .iter()
            .flat_map(|p| p.runs.iter().map(|r| r.text.as_str()))
            .collect::<String>()
    });
    Ok(AddShape {
        shape_type,
        left: dto.left,
        top: dto.top,
        width: dto.width,
        height: dto.height,
        text,
        // 0 is the sentinel the markdown parser emits for the omitted shape_id;
        // it means "assign the next free id".
        shape_id: (dto.shape_id != 0).then_some(dto.shape_id),
        name: dto.name.clone(),
        auto_shape_type: dto.auto_shape_type.clone(),
        table: dto.table.clone(),
        fill: dto.fill.clone(),
        outline: dto.outline.clone(),
    })
}

fn add_shape_to_part(xml: &[u8], shape_idx: usize, value: &Value) -> AppResult<Vec<u8>> {
    let dto: ShapeDto = serde_json::from_value(value.clone())
        .map_err(|e| AppError::InvalidValue(format!("Invalid shape JSON: {e}")))?;
    let add = shape_dto_to_add(&dto)?;
    let max_id = factory::find_max_shape_id(xml);
    let new_id = add.shape_id.unwrap_or(max_id + 1);
    let new_shape_xml = factory::generate_shape_xml(&add, new_id)?;
    editor::insert_shape_at(xml, shape_idx, &new_shape_xml)
}

// -- notes slide ------------------------------------------------------------

fn delete_notes_slide(pkg: &mut Package, slide_uri: &str) -> AppResult<()> {
    let Some(notes_uri) = crate::model::notes::resolve_notes_uri(pkg, slide_uri) else {
        return Ok(());
    };
    let rel_id = pkg.get_rels(slide_uri).and_then(|rels| {
        rels.values()
            .find(|r| r.rel_type == crate::model::notes::NOTES_SLIDE_REL_TYPE)
            .map(|r| r.id.clone())
    });
    if let Some(id) = rel_id {
        pkg.remove_relationship(slide_uri, &id);
    }
    pkg.remove_all_relationships(&notes_uri);
    pkg.remove_part(&notes_uri);
    pkg.remove_content_type_override(&format!("/{notes_uri}"))?;
    Ok(())
}

fn create_notes_slide(pkg: &mut Package, slide_uri: &str, value: &Value) -> AppResult<()> {
    if crate::model::notes::resolve_notes_uri(pkg, slide_uri).is_some() {
        return Err(AppError::InvalidValue(
            "Slide already has a notes slide".to_string(),
        ));
    }
    let slide_dto: crate::dto::SlideDto = serde_json::from_value({
        let mut value = value.clone();
        if let Some(shapes) = value.get_mut("shapes").and_then(|s| s.as_array_mut()) {
            for shape in shapes.iter_mut() {
                *shape = update_diff::insert_shape_defaults(shape);
            }
        }
        value
    })
    .map_err(|e| AppError::InvalidValue(format!("Invalid notes JSON: {e}")))?;

    let notes_num = pkg.get_next_notes_num();
    let notes_uri = format!("ppt/notesSlides/notesSlide{}.xml", notes_num);
    let notes_xml = factory::generate_notes_xml(&slide_dto)?;
    pkg.set_part(&notes_uri, notes_xml);
    pkg.add_content_type_override(
        &format!("/{notes_uri}"),
        "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml",
    )?;
    pkg.add_relationship(
        slide_uri,
        crate::opc::relationship::Relationship {
            id: String::new(),
            target: format!(
                "../{}",
                notes_uri.strip_prefix("ppt/").unwrap_or(&notes_uri)
            ),
            target_mode: None,
            rel_type: crate::model::notes::NOTES_SLIDE_REL_TYPE.to_string(),
        },
    );
    Ok(())
}
