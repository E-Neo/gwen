use super::*;

pub fn replace_shape_property_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    if remaining.is_empty() {
        return Err(AppError::PathParse("Empty property path".to_string()));
    }

    if remaining.len() == 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "text_frame")
    {
        return replace_text_frame_json_lossless(xml_bytes, shape_idx, value);
    }

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;

    edit_txbody_path(&mut events, txbody_start, txbody_end, remaining, value)?;
    write_events(events)
}

/// Replace the entire text frame of a shape with rich content from a JSON
/// `TextFrameDto` (paragraphs, runs, per-run fonts, hyperlinks, body properties).
/// Only the `p:txBody` subtree is replaced; the rest of the shape is untouched.
fn replace_text_frame_json_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let tf: crate::dto::TextFrameDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid text_frame JSON: {e}")))?;
    let inner_xml = crate::dto::xml::txbody_to_xml(&tf);
    let inner_events = read_events(inner_xml.as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;

    events.splice(txbody_start + 1..txbody_end, inner_events);
    write_events(events)
}

/// Replace the plain text of a shape, rebuilding its txBody with a single
/// paragraph/run containing `value`. Unlike patching existing `<a:t>` elements
/// in place, this also creates text when the shape has an empty text frame and
/// collapses multiple runs/paragraphs down to one.
pub fn replace_shape_text_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let tf: crate::dto::TextFrameDto = serde_json::from_value(json!({
        "paragraphs": [{ "runs": [{ "text": value }] }]
    }))
    .map_err(|e| AppError::InvalidValue(format!("Invalid text value: {e}")))?;
    let inner_xml = crate::dto::xml::txbody_to_xml(&tf);
    let inner_events = read_events(inner_xml.as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;

    events.splice(txbody_start + 1..txbody_end, inner_events);
    write_events(events)
}

/// Set one crop side (left/top/right/bottom) of a picture shape's `a:srcRect`
/// child of its `a:blip`. `value` is a fraction 0.0-1.0 of the amount cropped.
/// The `a:srcRect` element is created if the picture has none.
pub fn replace_picture_crop(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let side_attr = match remaining.get(1) {
        Some(path::PathSegment::Field(name)) => match name.as_str() {
            "left" => b"l",
            "top" => b"t",
            "right" => b"r",
            "bottom" => b"b",
            other => {
                return Err(AppError::PathParse(format!(
                    "Unsupported crop side '{other}'"
                )));
            }
        },
        _ => {
            return Err(AppError::PathParse(
                "Expected crop side after 'crop' (left/top/right/bottom)".to_string(),
            ));
        }
    };

    let fraction = value
        .parse::<f64>()
        .map_err(|_| AppError::InvalidValue(format!("Invalid crop value '{value}'")))?;
    let percent = (fraction * 100000.0).round() as i64;
    let percent_str = percent.to_string();

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;

    // Locate the a:blip inside the picture shape.
    let blip_range = {
        let mut i = shape_start;
        let mut blip: Option<(usize, usize)> = None;
        while i < shape_end {
            match &events[i] {
                Event::Start(e) if e.name().as_ref() == b"a:blip" => {
                    blip = find_elem_range(&events, b"a:blip", i);
                    break;
                }
                Event::Empty(e) if e.name().as_ref() == b"a:blip" => {
                    blip = Some((i, i));
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        blip
    }
    .ok_or(AppError::PathParse(
        "Shape at index has no a:blip (not a picture)".to_string(),
    ))?;

    let (blip_start, blip_end) = blip_range;

    // Locate an existing a:srcRect child of the blip.
    let src_range = find_child_elem_range(&events, blip_start, blip_end, b"a:srcRect");

    if let Some((s, e)) = src_range {
        if s == e {
            if let Event::Empty(ref orig) = events[s] {
                events[s] = Event::Empty(copy_attrs(orig, side_attr, &percent_str));
            }
        } else if let Event::Start(ref orig) = events[s] {
            events[s] = Event::Start(copy_attrs(orig, side_attr, &percent_str));
        }
    } else {
        // No a:srcRect yet: expand the blip into a start/end pair if it is
        // self-closing, then insert a fresh a:srcRect as its first child.
        let mut src = BytesStart::new("a:srcRect");
        let key_str = String::from_utf8_lossy(side_attr).to_string();
        src.push_attribute((key_str.as_str(), percent_str.as_str()));
        let insert_at = expand_empty_container(&mut events, blip_start, b"a:blip");
        events.insert(insert_at, Event::Empty(src));
    }

    write_events(events)
}

// ---------------------------------------------------------------------------
// Shape fill / outline (lossless)
// ---------------------------------------------------------------------------

const SHAPE_FILL_TAGS: [&[u8]; 6] = [
    b"a:solidFill",
    b"a:noFill",
    b"a:gradFill",
    b"a:blipFill",
    b"a:pattFill",
    b"a:grpFill",
];

const SPPR_LEADING_TAGS: [&[u8]; 3] = [b"a:xfrm", b"a:prstGeom", b"a:custGeom"];

fn find_sppr_range(
    events: &[Event<'_>],
    shape_start: usize,
    shape_end: usize,
) -> Option<(usize, usize)> {
    find_elem_range(events, b"p:spPr", shape_start).filter(|r| r.0 <= shape_end)
}

/// Compute the child index inside `p:spPr` where a fill/outline element should
/// be inserted: after every direct child whose tag is in `after`, before any
/// other child. Honors DrawingML child ordering (geometry, fill, line, effects).
fn sppr_insert_slot(
    events: &[Event<'_>],
    sppr_start: usize,
    sppr_end: usize,
    after: &[&[u8]],
) -> usize {
    let mut slot = sppr_start + 1;
    let mut i = sppr_start + 1;
    while i < sppr_end {
        match &events[i] {
            Event::Start(e) => {
                let name = e.name().as_ref().to_vec();
                if after.contains(&name.as_slice())
                    && let Some((_, re)) = find_elem_range(events, &name, i)
                {
                    slot = re + 1;
                    i = re + 1;
                    continue;
                }
                i += 1;
            }
            Event::Empty(e) => {
                if after.contains(&e.name().as_ref()) {
                    slot = i + 1;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    slot
}

fn fill_xml_events(fill: &crate::dto::FillDto) -> Vec<Event<'static>> {
    if matches!(fill.fill_type, Some(crate::dto::FillType::NoFill)) {
        return vec![Event::Empty(BytesStart::new("a:noFill"))];
    }
    let mut out = Vec::new();
    out.push(Event::Start(BytesStart::new("a:solidFill")));
    let color = fill.color.as_ref();
    let (tag, val): (&str, &str) = match color {
        Some(c) if c.rgb.is_some() => ("a:srgbClr", c.rgb.as_deref().unwrap()),
        Some(c) if c.theme_color.is_some() => ("a:schemeClr", c.theme_color.as_deref().unwrap()),
        _ => ("a:srgbClr", "4472C4"),
    };
    let mut clr = BytesStart::new(tag);
    clr.push_attribute(("val", val));
    out.push(Event::Empty(clr));
    out.push(Event::End(BytesEnd::new("a:solidFill")));
    out
}

fn outline_xml_events(outline: &crate::dto::OutlineDto) -> Vec<Event<'static>> {
    if outline.width.is_none()
        && outline.cap.is_none()
        && outline.compound.is_none()
        && outline.dash.is_none()
        && outline.fill.is_none()
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut ln = BytesStart::new("a:ln");
    if let Some(w) = outline.width {
        ln.push_attribute(("w", w.to_string().as_str()));
    }
    if let Some(cap) = &outline.cap {
        ln.push_attribute((
            "cap",
            match cap {
                crate::dto::LineCap::Rnd => "rnd",
                crate::dto::LineCap::Sq => "sq",
                crate::dto::LineCap::Flat => "flat",
            },
        ));
    }
    if let Some(cmp) = &outline.compound {
        ln.push_attribute((
            "cmpd",
            match cmp {
                crate::dto::CompoundLine::Sng => "sng",
                crate::dto::CompoundLine::Dbl => "dbl",
                crate::dto::CompoundLine::ThickThin => "thickThin",
                crate::dto::CompoundLine::ThinThick => "thinThick",
                crate::dto::CompoundLine::Tri => "tri",
            },
        ));
    }
    out.push(Event::Start(ln));
    if let Some(dash) = &outline.dash {
        let mut d = BytesStart::new("a:prstDash");
        d.push_attribute((
            "val",
            match dash {
                crate::dto::LineDash::Solid => "solid",
                crate::dto::LineDash::Dot => "dot",
                crate::dto::LineDash::Dash => "dash",
                crate::dto::LineDash::LgDash => "lgDash",
                crate::dto::LineDash::DashDot => "dashDot",
                crate::dto::LineDash::LgDashDot => "lgDashDot",
                crate::dto::LineDash::LgDashDotDot => "lgDashDotDot",
                crate::dto::LineDash::SysDash => "sysDash",
                crate::dto::LineDash::SysDot => "sysDot",
                crate::dto::LineDash::SysDashDot => "sysDashDot",
                crate::dto::LineDash::SysDashDotDot => "sysDashDotDot",
            },
        ));
        out.push(Event::Empty(d));
    }
    if let Some(fill) = &outline.fill {
        out.extend(fill_xml_events(fill));
    }
    out.push(Event::End(BytesEnd::new("a:ln")));
    out
}

fn color_dto_from_value(
    rest: &[path::PathSegment],
    value: &str,
) -> AppResult<crate::dto::ColorFormatDto> {
    let path::PathSegment::Field(name) = &rest[0] else {
        return Err(AppError::PathParse(
            "Expected color property name".to_string(),
        ));
    };
    match name.as_str() {
        "rgb" => Ok(crate::dto::ColorFormatDto {
            color_type: Some(crate::dto::ColorType::Rgb),
            rgb: Some(value.to_string()),
            theme_color: None,
        }),
        "theme_color" => Ok(crate::dto::ColorFormatDto {
            color_type: Some(crate::dto::ColorType::Scheme),
            rgb: None,
            theme_color: Some(value.to_string()),
        }),
        _ => Err(AppError::PathParse(format!(
            "Unknown color property '{}'",
            name.as_str()
        ))),
    }
}

fn parse_dto_enum<T: serde::de::DeserializeOwned>(value: &str) -> Option<T> {
    serde_json::from_str(value)
        .ok()
        .or_else(|| serde_json::from_str(&format!("\"{value}\"")).ok())
}

fn fill_dto_from_value(rest: &[path::PathSegment], value: &str) -> AppResult<crate::dto::FillDto> {
    if rest.is_empty() {
        return serde_json::from_str(value)
            .map_err(|e| AppError::InvalidValue(format!("Invalid fill JSON: {e}")));
    }
    let path::PathSegment::Field(name) = &rest[0] else {
        return Err(AppError::PathParse(
            "Expected fill property name".to_string(),
        ));
    };
    match name.as_str() {
        "type" => {
            let t = parse_dto_enum::<crate::dto::FillType>(value)
                .or(match value {
                    "nofill" | "none" => Some(crate::dto::FillType::NoFill),
                    _ => None,
                })
                .ok_or_else(|| AppError::InvalidValue(format!("Invalid fill type '{value}'")))?;
            Ok(crate::dto::FillDto {
                fill_type: Some(t),
                color: None,
            })
        }
        "color" => {
            let color = if rest.len() == 1 {
                serde_json::from_str(value)
                    .map_err(|e| AppError::InvalidValue(format!("Invalid color JSON: {e}")))?
            } else {
                color_dto_from_value(&rest[1..], value)?
            };
            Ok(crate::dto::FillDto {
                fill_type: Some(crate::dto::FillType::Solid),
                color: Some(color),
            })
        }
        _ => Err(AppError::PathParse(format!(
            "Unknown fill property '{}'",
            name.as_str()
        ))),
    }
}

fn outline_dto_from_value(
    rest: &[path::PathSegment],
    value: &str,
) -> AppResult<crate::dto::OutlineDto> {
    let mut outline = crate::dto::OutlineDto {
        width: None,
        cap: None,
        compound: None,
        dash: None,
        fill: None,
    };
    if rest.is_empty() {
        return serde_json::from_str(value)
            .map_err(|e| AppError::InvalidValue(format!("Invalid outline JSON: {e}")));
    }
    let path::PathSegment::Field(name) = &rest[0] else {
        return Err(AppError::PathParse(
            "Expected outline property name".to_string(),
        ));
    };
    match name.as_str() {
        "width" => {
            outline.width = Some(
                value
                    .parse()
                    .map_err(|_| AppError::InvalidValue(format!("Invalid width '{value}'")))?,
            );
        }
        "cap" => {
            outline.cap = Some(
                parse_dto_enum::<crate::dto::LineCap>(value)
                    .ok_or_else(|| AppError::InvalidValue(format!("Invalid cap '{value}'")))?,
            );
        }
        "compound" => {
            outline.compound = Some(
                parse_dto_enum::<crate::dto::CompoundLine>(value)
                    .ok_or_else(|| AppError::InvalidValue(format!("Invalid compound '{value}'")))?,
            );
        }
        "dash" => {
            outline.dash = Some(
                parse_dto_enum::<crate::dto::LineDash>(value)
                    .ok_or_else(|| AppError::InvalidValue(format!("Invalid dash '{value}'")))?,
            );
        }
        "fill" => {
            let fill = if rest.len() == 1 {
                fill_dto_from_value(&[], value)?
            } else {
                fill_dto_from_value(&rest[1..], value)?
            };
            outline.fill = Some(fill);
        }
        _ => {
            return Err(AppError::PathParse(format!(
                "Unknown outline property '{}'",
                name.as_str()
            )));
        }
    }
    Ok(outline)
}

/// Set the fill of an existing shape (`fill`, `fill.type`, `fill.color`,
/// `fill.color.rgb`, `fill.color.theme_color`). Only `p:sp`, `p:pic` and
/// `p:cxnSp` have a fill; other shape types error.
pub fn replace_shape_fill_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let rest = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "fill") {
        &remaining[1..]
    } else {
        remaining
    };
    let fill = fill_dto_from_value(rest, value)?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (sppr_start, sppr_end) = find_sppr_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no properties element".to_string()))?;

    remove_children_by_name(&mut events, sppr_start, sppr_end, &SHAPE_FILL_TAGS);

    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (sppr_start, sppr_end) = find_sppr_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no properties element".to_string()))?;
    let slot = sppr_insert_slot(&events, sppr_start, sppr_end, &SPPR_LEADING_TAGS);

    for ev in fill_xml_events(&fill).into_iter().rev() {
        events.insert(slot, ev);
    }
    write_events(events)
}

/// Set the outline (line) of an existing shape (`outline`, `outline.width`,
/// `outline.dash`, `outline.cap`, `outline.compound`, `outline.fill`, …).
pub fn replace_shape_outline_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let rest = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "outline") {
        &remaining[1..]
    } else {
        remaining
    };
    let outline = outline_dto_from_value(rest, value)?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (sppr_start, sppr_end) = find_sppr_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no properties element".to_string()))?;

    remove_children_by_name(&mut events, sppr_start, sppr_end, &[b"a:ln"]);

    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (sppr_start, sppr_end) = find_sppr_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no properties element".to_string()))?;
    let mut after: Vec<&[u8]> = SPPR_LEADING_TAGS.to_vec();
    after.extend(SHAPE_FILL_TAGS);
    let slot = sppr_insert_slot(&events, sppr_start, sppr_end, &after);

    for ev in outline_xml_events(&outline).into_iter().rev() {
        events.insert(slot, ev);
    }
    write_events(events)
}

/// Remove the text frame (`p:txBody`) from a shape.
pub fn delete_shape_text_frame(xml_bytes: &[u8], shape_idx: usize) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let Some((s, e)) =
        find_elem_range(&events, b"p:txBody", shape_start).filter(|r| r.0 <= shape_end)
    else {
        return Err(AppError::PathParse("Shape has no text frame".to_string()));
    };
    for j in (s..=e).rev() {
        events.remove(j);
    }
    write_events(events)
}

/// Remove the fill child elements from a shape's `p:spPr`. When the shape has
/// no local property element (the fill is inherited from the layout), there is
/// nothing to remove and the document is returned unchanged.
pub fn delete_shape_fill(xml_bytes: &[u8], shape_idx: usize) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let Some((sppr_start, sppr_end)) = find_sppr_range(&events, shape_start, shape_end) else {
        return Ok(xml_bytes.to_vec());
    };
    remove_children_by_name(&mut events, sppr_start, sppr_end, &SHAPE_FILL_TAGS);
    write_events(events)
}

/// Remove the outline (`a:ln`) from a shape's `p:spPr`, no-op when the shape
/// has no local properties element.
pub fn delete_shape_outline(xml_bytes: &[u8], shape_idx: usize) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let Some((sppr_start, sppr_end)) = find_sppr_range(&events, shape_start, shape_end) else {
        return Ok(xml_bytes.to_vec());
    };
    remove_children_by_name(&mut events, sppr_start, sppr_end, &[b"a:ln"]);
    write_events(events)
}

/// Remove one crop side from a picture's `a:srcRect`, restoring full display.
pub fn delete_picture_crop_side(
    xml_bytes: &[u8],
    shape_idx: usize,
    side: &str,
) -> AppResult<Vec<u8>> {
    let attr_key: &[u8] = match side {
        "left" => b"l",
        "top" => b"t",
        "right" => b"r",
        "bottom" => b"b",
        other => {
            return Err(AppError::PathParse(format!(
                "Unsupported crop side '{other}'"
            )));
        }
    };
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let src_rect = {
        let mut i = shape_start;
        let mut found: Option<(usize, usize)> = None;
        while i < shape_end {
            match &events[i] {
                Event::Start(e) if e.name().as_ref() == b"a:srcRect" => {
                    found = find_elem_range(&events, b"a:srcRect", i);
                    break;
                }
                Event::Empty(e) if e.name().as_ref() == b"a:srcRect" => {
                    found = Some((i, i));
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        found
    };
    match src_rect {
        Some((s, e)) if s == e => {
            if let Event::Empty(orig) = &events[s] {
                events[s] = Event::Empty(remove_attr_from_start(orig, attr_key));
            }
        }
        Some((s, _e)) => {
            if let Event::Start(orig) = &events[s] {
                events[s] = Event::Start(remove_attr_from_start(orig, attr_key));
            }
        }
        None => {}
    }
    write_events(events)
}

/// Set the `prst` attribute of a shape's `a:prstGeom` (the `auto_shape_type`
/// field).
pub fn set_auto_shape_type_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let geom = find_elem_range(&events, b"a:prstGeom", shape_start).filter(|r| r.0 <= shape_end);
    match geom {
        Some((s, e)) => {
            let (Event::Empty(orig) | Event::Start(orig)) = &events[s] else {
                return Err(AppError::PathParse(
                    "Shape has no prstGeom element".to_string(),
                ));
            };
            let name = String::from_utf8_lossy(orig.name().as_ref()).to_string();
            let mut elem = BytesStart::new(name);
            let mut set = false;
            for attr in orig.attributes().flatten() {
                if attr.key.as_ref() == b"prst" {
                    elem.push_attribute(("prst", value));
                    set = true;
                } else {
                    let ak = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let av = String::from_utf8_lossy(&attr.value).to_string();
                    elem.push_attribute((ak.as_str(), av.as_str()));
                }
            }
            if !set {
                elem.push_attribute(("prst", value));
            }
            events[s] = if e == s {
                Event::Empty(elem)
            } else {
                Event::Start(elem)
            };
        }
        None => {
            return Err(AppError::PathParse(
                "Shape has no prstGeom element".to_string(),
            ));
        }
    }
    write_events(events)
}

/// Remove the `a:srcRect` (whole crop) from a picture shape.
pub fn delete_picture_crop(xml_bytes: &[u8], shape_idx: usize) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let src_rect = {
        let mut i = shape_start;
        let mut found: Option<(usize, usize)> = None;
        while i < shape_end {
            match &events[i] {
                Event::Start(e) if e.name().as_ref() == b"a:srcRect" => {
                    found = find_elem_range(&events, b"a:srcRect", i);
                    break;
                }
                Event::Empty(e) if e.name().as_ref() == b"a:srcRect" => {
                    found = Some((i, i));
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        found
    };
    if let Some((s, e)) = src_rect {
        for j in (s..=e).rev() {
            events.remove(j);
        }
    }
    write_events(events)
}

/// Replace a shape's entire text frame with freshly serialized `TextFrameDto`,
/// creating the `p:txBody` element when the shape has none.
pub fn replace_or_create_text_frame_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let tf: crate::dto::TextFrameDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid text_frame JSON: {e}")))?;
    let inner_events = read_events(crate::dto::xml::txbody_to_xml(&tf).as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;

    match find_txbody_range(&events, shape_start, shape_end) {
        Some((txbody_start, txbody_end)) => {
            events.splice(txbody_start + 1..txbody_end, inner_events);
        }
        None => {
            // Find the end of the shape's spPr block to insert txBody after it.
            let sppr = find_elem_range(&events, b"p:spPr", shape_start)
                .filter(|r| r.0 <= shape_end)
                .or_else(|| {
                    find_elem_range(&events, b"p:blipFill", shape_start)
                        .filter(|r| r.0 <= shape_end)
                });
            let insert_at = match sppr {
                Some((_, e)) => e + 1,
                None => shape_start + 1,
            };
            events.insert(insert_at, Event::End(BytesEnd::new("p:txBody")));
            for ev in inner_events.into_iter().rev() {
                events.insert(insert_at, ev);
            }
            events.insert(insert_at, Event::Start(BytesStart::new("p:txBody")));
        }
    }
    write_events(events)
}
