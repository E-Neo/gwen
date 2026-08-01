use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::escape::escape;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::io::Write;

use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::path;

pub(crate) fn read_events(xml_bytes: &[u8]) -> AppResult<Vec<Event<'static>>> {
    let mut reader = Reader::from_reader(xml_bytes);
    let mut events = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Err(e) => return Err(AppError::Xml(e)),
            Ok(e) => events.push(e.into_owned()),
        }
    }
    Ok(events)
}

pub(crate) fn write_events(events: Vec<Event<'static>>) -> AppResult<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    for e in events {
        writer.write_event(e).map_err(AppError::Io)?;
    }
    Ok(writer.into_inner())
}

fn is_shape_tag(name: &[u8]) -> bool {
    matches!(
        name,
        b"p:sp" | b"p:pic" | b"p:cxnSp" | b"p:grpSp" | b"p:graphicFrame"
    )
}

pub(crate) fn find_elem_range(
    events: &[Event<'_>],
    name: &[u8],
    start_from: usize,
) -> Option<(usize, usize)> {
    let mut depth = 0u32;
    let mut start = None;
    for (i, event) in events.iter().enumerate().skip(start_from) {
        match event {
            Event::Start(e) => {
                if start.is_none() && e.name().as_ref() == name {
                    start = Some((i, depth));
                }
                depth += 1;
            }
            Event::End(_) => {
                if let Some((s, start_depth)) = start
                    && depth == start_depth + 1
                {
                    return Some((s, i));
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    None
}

fn find_nth_child_range(
    events: &[Event<'_>],
    parent_start: usize,
    parent_end: usize,
    child_name: &[u8],
    n: usize,
) -> Option<(usize, usize)> {
    let mut count = 0usize;
    let mut depth = 0u32;
    let mut i = parent_start + 1;
    while i < parent_end {
        match &events[i] {
            Event::Start(e) => {
                if depth == 0 && e.name().as_ref() == child_name {
                    if count == n {
                        return find_elem_range(events, child_name, i);
                    }
                    count += 1;
                }
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Find the first direct child element by name (Start or self-closing) inside a
/// parent range. Returns `(start, end)`; for a self-closing element `start == end`.
pub(crate) fn find_child_elem_range(
    events: &[Event<'_>],
    parent_start: usize,
    parent_end: usize,
    child_name: &[u8],
) -> Option<(usize, usize)> {
    let mut depth = 0u32;
    let mut i = parent_start + 1;
    while i < parent_end {
        match &events[i] {
            Event::Start(e) => {
                if depth == 0 && e.name().as_ref() == child_name {
                    return find_elem_range(events, child_name, i);
                }
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
            }
            Event::Empty(e) if depth == 0 && e.name().as_ref() == child_name => {
                return Some((i, i));
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn find_nth_elem_range(
    events: &[Event<'_>],
    name: &[u8],
    start: usize,
    end: usize,
    n: usize,
) -> Option<(usize, usize)> {
    let mut count = 0usize;
    let mut i = start;
    while i <= end {
        if let Event::Start(e) = &events[i]
            && e.name().as_ref() == name
        {
            if count == n {
                return find_elem_range(events, name, i);
            }
            count += 1;
        }
        i += 1;
    }
    None
}

fn find_shape_range(events: &[Event<'_>], shape_idx: usize) -> Option<(usize, usize)> {
    let mut count = 0usize;
    let mut i = 0;
    while i < events.len() {
        if let Event::Start(e) = &events[i]
            && is_shape_tag(e.name().as_ref())
        {
            if count == shape_idx {
                return find_elem_range(events, e.name().as_ref(), i);
            }
            count += 1;
        }
        i += 1;
    }
    None
}

fn find_txbody_range(
    events: &[Event<'_>],
    shape_start: usize,
    shape_end: usize,
) -> Option<(usize, usize)> {
    if let Some(r) = find_elem_range(events, b"p:txBody", shape_start)
        && r.0 <= shape_end
    {
        return Some(r);
    }
    if let Some(r) = find_elem_range(events, b"a:txBody", shape_start)
        && r.0 <= shape_end
    {
        return Some(r);
    }
    None
}

fn copy_attrs(e: &BytesStart<'_>, key: &[u8], val: &str) -> BytesStart<'static> {
    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
    let mut elem = BytesStart::new(name);
    let mut found = false;
    for attr in e.attributes().flatten() {
        let ak = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let av = String::from_utf8_lossy(&attr.value).to_string();
        if attr.key.as_ref() == key {
            elem.push_attribute((ak.as_str(), val));
            found = true;
        } else {
            elem.push_attribute((ak.as_str(), av.as_str()));
        }
    }
    if !found {
        let key_str = String::from_utf8_lossy(key).to_string();
        elem.push_attribute((key_str.as_str(), val));
    }
    elem
}

fn copy_attrs_all(e: &BytesStart<'_>) -> BytesStart<'static> {
    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
    let mut elem = BytesStart::new(name);
    for attr in e.attributes().flatten() {
        let ak = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let av = String::from_utf8_lossy(&attr.value).to_string();
        elem.push_attribute((ak.as_str(), av.as_str()));
    }
    elem
}

fn expand_empty_container(events: &mut Vec<Event<'static>>, s: usize, name: &[u8]) -> usize {
    if let Event::Empty(e) = &events[s]
        && e.name().as_ref() == name
    {
        events[s] = Event::Start(copy_attrs_all(e));
        events.insert(
            s + 1,
            Event::End(BytesEnd::new(String::from_utf8_lossy(name).into_owned())),
        );
        return s + 1;
    }
    s + 1
}

fn is_whitespace_text(event: &Event<'_>) -> bool {
    let Event::Text(t) = event else {
        return false;
    };
    t.iter().all(|b| b.is_ascii_whitespace())
}

fn find_rpr_in_run(
    events: &[Event<'_>],
    run_start: usize,
    run_end: usize,
) -> (Option<(usize, usize)>, usize) {
    let mut i = run_start + 1;
    while i < run_end {
        match &events[i] {
            Event::Start(e) if e.name().as_ref() == b"a:rPr" => {
                if let Some(range) = find_elem_range(events, b"a:rPr", i) {
                    return (Some(range), i);
                }
            }
            Event::Empty(e) if e.name().as_ref() == b"a:rPr" => {
                return (Some((i, i)), i);
            }
            Event::Text(_) if is_whitespace_text(&events[i]) => {}
            Event::Start(_) | Event::Empty(_) | Event::Text(_) => {
                return (None, i);
            }
            _ => {}
        }
        i += 1;
    }
    (None, run_start + 1)
}

fn find_ppr_in_para(
    events: &[Event<'_>],
    para_start: usize,
    para_end: usize,
) -> (Option<(usize, usize)>, usize) {
    let mut i = para_start + 1;
    while i < para_end {
        match &events[i] {
            Event::Start(e) if e.name().as_ref() == b"a:pPr" => {
                if let Some(range) = find_elem_range(events, b"a:pPr", i) {
                    return (Some(range), i);
                }
            }
            Event::Empty(e) if e.name().as_ref() == b"a:pPr" => {
                return (Some((i, i)), i);
            }
            Event::Text(_) if is_whitespace_text(&events[i]) => {}
            Event::Start(_) | Event::Empty(_) | Event::Text(_) => {
                return (None, i);
            }
            _ => {}
        }
        i += 1;
    }
    (None, para_start + 1)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

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
            brightness: None,
        }),
        "theme_color" => Ok(crate::dto::ColorFormatDto {
            color_type: Some(crate::dto::ColorType::Scheme),
            rgb: None,
            theme_color: Some(value.to_string()),
            brightness: None,
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
                alpha: None,
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
                alpha: None,
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

fn edit_txbody_path(
    events: &mut Vec<Event<'static>>,
    txbody_start: usize,
    txbody_end: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<()> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "text_frame") {
        &remaining[1..]
    } else {
        remaining
    };

    if inner.is_empty() {
        return Err(AppError::PathParse("Empty text_frame path".to_string()));
    }

    match &inner[0] {
        path::PathSegment::Field(name) if name == "text" => {
            edit_run_text_in_place(events, txbody_start, txbody_end, 0, 0, value)
        }
        path::PathSegment::Field(name) if name == "paragraphs" => {
            if inner.len() < 2 {
                return Err(AppError::PathParse("Paragraph index required".to_string()));
            }
            let para_idx = match &inner[1] {
                path::PathSegment::Index(i) => *i,
                _ => {
                    return Err(AppError::PathParse(
                        "Expected paragraph index after 'paragraphs'".to_string(),
                    ));
                }
            };
            let rest = &inner[2..];

            match rest.split_first() {
                Some((path::PathSegment::Field(n), tail)) if n == "runs" => {
                    let run_idx = match tail.first() {
                        Some(path::PathSegment::Index(i)) => *i,
                        _ => return Err(AppError::PathParse("Expected run index".to_string())),
                    };
                    match tail.get(1) {
                        Some(path::PathSegment::Field(n)) if n == "font" => edit_run_font_in_place(
                            events,
                            txbody_start,
                            txbody_end,
                            para_idx,
                            run_idx,
                            &tail[2..],
                            value,
                        ),
                        Some(path::PathSegment::Field(n)) if n == "text" => edit_run_text_in_place(
                            events,
                            txbody_start,
                            txbody_end,
                            para_idx,
                            run_idx,
                            value,
                        ),
                        _ => Err(AppError::PathParse(
                            "Expected 'font' or 'text' after run index".to_string(),
                        )),
                    }
                }
                Some((path::PathSegment::Field(n), tail)) if n == "font" => {
                    edit_end_para_rpr_in_place(
                        events,
                        txbody_start,
                        txbody_end,
                        para_idx,
                        tail,
                        value,
                    )
                }
                _ => edit_paragraph_prop_in_place(
                    events,
                    txbody_start,
                    txbody_end,
                    para_idx,
                    rest,
                    value,
                ),
            }
        }
        _ => edit_txbody_prop_in_place(events, txbody_start, txbody_end, inner, value),
    }
}

fn edit_run_font_in_place(
    events: &mut Vec<Event<'static>>,
    txbody_start: usize,
    txbody_end: usize,
    para_idx: usize,
    run_idx: usize,
    font_path: &[path::PathSegment],
    value: &str,
) -> AppResult<()> {
    if font_path.is_empty() {
        return Err(AppError::PathParse("Font property required".to_string()));
    }
    let prop = match &font_path[0] {
        path::PathSegment::Field(n) => n.clone(),
        _ => {
            return Err(AppError::PathParse(
                "Expected font property name".to_string(),
            ));
        }
    };

    let (para_start, para_end) =
        find_nth_child_range(events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;
    let (run_start, run_end) = find_nth_child_range(events, para_start, para_end, b"a:r", run_idx)
        .ok_or_else(|| AppError::PathParse(format!("Run index {run_idx} not found")))?;

    let (rpr_range, insert_pos) = find_rpr_in_run(events, run_start, run_end);

    match prop.as_str() {
        "size" | "bold" | "italic" | "underline" => {
            let attr_val = match prop.as_str() {
                "bold" | "italic" => match value {
                    "true" | "1" => "1",
                    "false" | "0" => "0",
                    _ => {
                        return Err(AppError::InvalidValue(format!(
                            "Invalid {prop}: use true/false or 1/0"
                        )));
                    }
                },
                "underline" => match value {
                    "true" | "1" => "sng",
                    "false" | "0" => "none",
                    other => other,
                },
                _ => value,
            };
            let key: &[u8] = match prop.as_str() {
                "size" => b"sz",
                "bold" => b"b",
                "italic" => b"i",
                _ => b"u",
            };
            set_rpr_attr(events, &rpr_range, insert_pos, key, attr_val);
        }
        "name" => set_rpr_latin(events, &rpr_range, insert_pos, value),
        "color" => set_rpr_color(events, &rpr_range, insert_pos, value),
        _ => {
            return Err(AppError::PathParse(format!(
                "Unknown font property '{prop}'"
            )));
        }
    }

    Ok(())
}

fn edit_run_text_in_place(
    events: &mut Vec<Event<'static>>,
    txbody_start: usize,
    txbody_end: usize,
    para_idx: usize,
    run_idx: usize,
    value: &str,
) -> AppResult<()> {
    let (para_start, para_end) =
        find_nth_child_range(events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;
    let (run_start, run_end) = find_nth_child_range(events, para_start, para_end, b"a:r", run_idx)
        .ok_or_else(|| AppError::PathParse(format!("Run index {run_idx} not found")))?;

    let t_range = find_elem_range(events, b"a:t", run_start).filter(|r| r.0 <= run_end);
    match t_range {
        Some((s, e)) => {
            replace_text_in_range(events, s, e, value);
        }
        None => {
            events.insert(run_end, Event::End(BytesEnd::new("a:t")));
            events.insert(
                run_end,
                Event::Text(BytesText::from_escaped(escape(value.to_string()))),
            );
            events.insert(run_end, Event::Start(BytesStart::new("a:t")));
        }
    }

    Ok(())
}

fn edit_end_para_rpr_in_place(
    events: &mut Vec<Event<'static>>,
    txbody_start: usize,
    txbody_end: usize,
    para_idx: usize,
    font_path: &[path::PathSegment],
    value: &str,
) -> AppResult<()> {
    if font_path.is_empty() {
        return Err(AppError::PathParse("Font property required".to_string()));
    }
    let prop = match &font_path[0] {
        path::PathSegment::Field(n) => n.clone(),
        _ => {
            return Err(AppError::PathParse(
                "Expected font property name".to_string(),
            ));
        }
    };

    let (para_start, para_end) =
        find_nth_child_range(events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;

    let end_rpr_range =
        find_elem_range(events, b"a:endParaRPr", para_start).filter(|r| r.0 <= para_end);

    let (rpr_range, insert_pos) = if let Some((s, e)) = end_rpr_range {
        (Some((s, e)), s + 1)
    } else {
        (None, para_end)
    };

    let key: &[u8] = match prop.as_str() {
        "size" => b"sz",
        "bold" => b"b",
        "italic" => b"i",
        "underline" => b"u",
        "name" | "color" => b"", // handled separately
        _ => {
            return Err(AppError::PathParse(format!(
                "Unknown font property '{prop}'"
            )));
        }
    };

    match prop.as_str() {
        "size" | "bold" | "italic" | "underline" => {
            let attr_val = match prop.as_str() {
                "bold" | "italic" => match value {
                    "true" | "1" => "1",
                    "false" | "0" => "0",
                    _ => return Err(AppError::InvalidValue(format!("Invalid {prop}"))),
                },
                "underline" => match value {
                    "true" | "1" => "sng",
                    "false" | "0" => "none",
                    other => other,
                },
                _ => value,
            };
            set_end_rpr_attr(events, &rpr_range, insert_pos, key, attr_val);
        }
        "name" => set_end_rpr_latin(events, &rpr_range, insert_pos, value),
        "color" => set_end_rpr_color(events, &rpr_range, insert_pos, value),
        _ => unreachable!(),
    }

    Ok(())
}

fn edit_paragraph_prop_in_place(
    events: &mut Vec<Event<'static>>,
    txbody_start: usize,
    txbody_end: usize,
    para_idx: usize,
    path_rest: &[path::PathSegment],
    value: &str,
) -> AppResult<()> {
    if path_rest.is_empty() {
        return Err(AppError::PathParse(
            "Paragraph property required".to_string(),
        ));
    }
    let prop = match &path_rest[0] {
        path::PathSegment::Field(n) => n.clone(),
        _ => {
            return Err(AppError::PathParse(
                "Expected paragraph property name".to_string(),
            ));
        }
    };

    let (para_start, para_end) =
        find_nth_child_range(events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;

    match prop.as_str() {
        "alignment" => {
            let (ppr_range, insert_pos) = find_ppr_in_para(events, para_start, para_end);
            set_ppr_attr(events, &ppr_range, insert_pos, b"algn", value);
        }
        "level" => {
            let (ppr_range, insert_pos) = find_ppr_in_para(events, para_start, para_end);
            set_ppr_attr(events, &ppr_range, insert_pos, b"lvl", value);
        }
        "line_spacing" => edit_spacing_elem(events, para_start, para_end, b"a:lnSpc", value, true),
        "space_before" => {
            edit_spacing_elem(events, para_start, para_end, b"a:spcBef", value, false)
        }
        "space_after" => edit_spacing_elem(events, para_start, para_end, b"a:spcAft", value, false),
        _ => {
            return Err(AppError::PathParse(format!(
                "Unknown paragraph property '{prop}'"
            )));
        }
    }

    Ok(())
}

fn edit_txbody_prop_in_place(
    events: &mut Vec<Event<'static>>,
    txbody_start: usize,
    txbody_end: usize,
    path_rest: &[path::PathSegment],
    value: &str,
) -> AppResult<()> {
    if path_rest.is_empty() {
        return Err(AppError::PathParse(
            "Text frame property required".to_string(),
        ));
    }
    let prop = match &path_rest[0] {
        path::PathSegment::Field(n) => n.clone(),
        _ => {
            return Err(AppError::PathParse(
                "Expected text frame property name".to_string(),
            ));
        }
    };

    let bodypr_range =
        find_elem_range(events, b"a:bodyPr", txbody_start).filter(|r| r.0 <= txbody_end);

    match prop.as_str() {
        "word_wrap" | "vertical_anchor" | "margin_left" | "margin_right" | "margin_top"
        | "margin_bottom" => {
            let attr_key: &[u8] = match prop.as_str() {
                "word_wrap" => b"wrap",
                "vertical_anchor" => b"anchor",
                "margin_left" => b"lIns",
                "margin_right" => b"rIns",
                "margin_top" => b"tIns",
                "margin_bottom" => b"bIns",
                _ => unreachable!(),
            };
            let attr_val = if prop.as_str() == "word_wrap" {
                match value {
                    "true" | "1" => "square",
                    "false" | "0" => "none",
                    other => other,
                }
            } else if prop.as_str() == "vertical_anchor" {
                match value {
                    "top" | "t" => "t",
                    "middle" | "ctr" => "ctr",
                    "bottom" | "b" => "b",
                    "justified" | "just" => "just",
                    "distributed" | "dist" => "dist",
                    other => other,
                }
            } else {
                value
            };

            if let Some((s, e)) = bodypr_range {
                if s == e {
                    if let Event::Empty(ref orig) = events[s] {
                        events[s] = Event::Empty(copy_attrs(orig, attr_key, attr_val));
                    }
                } else if let Event::Start(ref orig) = events[s] {
                    events[s] = Event::Start(copy_attrs(orig, attr_key, attr_val));
                }
            } else {
                let mut body_pr = BytesStart::new("a:bodyPr");
                let key_str = String::from_utf8_lossy(attr_key).to_string();
                body_pr.push_attribute((key_str.as_str(), attr_val));
                events.insert(txbody_start + 1, Event::Empty(body_pr));
            }
        }
        "auto_size" => {
            let child_name: &[u8] = match value {
                "text_to_fit_shape" | "TextToFitShape" => b"a:spAutoFit",
                "shape_to_fit_text" | "ShapeToFitText" => b"a:normAutofit",
                "none" | "None" => b"a:noAutofit",
                _ => {
                    return Err(AppError::InvalidValue(format!(
                        "Invalid auto_size '{value}'"
                    )));
                }
            };
            if let Some((s, e)) = bodypr_range {
                let auto_children: [&[u8]; 3] = [b"a:spAutoFit", b"a:normAutofit", b"a:noAutofit"];
                let mut to_remove: Vec<usize> = Vec::new();
                let mut j = s + 1;
                while j < e {
                    let should = match &events[j] {
                        Event::Start(ev) | Event::Empty(ev) => {
                            auto_children.contains(&ev.name().as_ref())
                        }
                        _ => false,
                    };
                    if should {
                        to_remove.push(j);
                    }
                    j += 1;
                }
                for idx in to_remove.into_iter().rev() {
                    events.remove(idx);
                }

                let insert_at = expand_empty_container(events, s, b"a:bodyPr");
                let child_name_owned = String::from_utf8_lossy(child_name).to_string();
                events.insert(insert_at, Event::Empty(BytesStart::new(child_name_owned)));
            } else {
                let child_name_owned = String::from_utf8_lossy(child_name).to_string();
                events.insert(txbody_start + 1, Event::End(BytesEnd::new("a:bodyPr")));
                events.insert(
                    txbody_start + 1,
                    Event::Empty(BytesStart::new(child_name_owned)),
                );
                events.insert(txbody_start + 1, Event::Start(BytesStart::new("a:bodyPr")));
            }
        }
        _ => {
            return Err(AppError::PathParse(format!(
                "Unknown text frame property '{prop}'"
            )));
        }
    }

    Ok(())
}

pub fn replace_table_cell_property_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };

    let (rest, row_idx, cell_idx) = match inner {
        [
            path::PathSegment::Field(rows),
            path::PathSegment::Index(row_idx),
            path::PathSegment::Field(cells),
            path::PathSegment::Index(cell_idx),
            rest @ ..,
        ] if rows.as_str() == "rows" && cells.as_str() == "cells" => (rest, *row_idx, *cell_idx),
        _ => {
            return Err(AppError::PathParse(
                "Expected table.rows[N].cells[M]".to_string(),
            ));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let tbl_range = find_elem_range(&events, b"a:tbl", shape_start)
        .filter(|r| r.0 <= shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no table".to_string()))?;
    let (row_start, row_end) =
        find_nth_child_range(&events, tbl_range.0, tbl_range.1, b"a:tr", row_idx)
            .ok_or_else(|| AppError::PathParse(format!("Table row {row_idx} not found")))?;
    let (cell_start, cell_end) =
        find_nth_child_range(&events, row_start, row_end, b"a:tc", cell_idx)
            .ok_or_else(|| AppError::PathParse(format!("Table cell {cell_idx} not found")))?;
    let (txbody_start, txbody_end) = find_elem_range(&events, b"a:txBody", cell_start)
        .filter(|r| r.0 <= cell_end)
        .ok_or_else(|| AppError::PathParse("Cell has no txBody".to_string()))?;

    if rest.len() == 1 && matches!(&rest[0], path::PathSegment::Field(n) if n == "text_frame") {
        let tf: crate::dto::TextFrameDto = serde_json::from_str(value)
            .map_err(|e| AppError::InvalidValue(format!("Invalid text_frame JSON: {e}")))?;
        let inner_events = read_events(crate::dto::xml::txbody_to_xml(&tf).as_bytes())?;
        events.splice(txbody_start + 1..txbody_end, inner_events);
        return write_events(events);
    }

    edit_txbody_path(&mut events, txbody_start, txbody_end, rest, value)?;
    write_events(events)
}

/// Generate the `<a:tr>` XML string for a new table row, padded with empty
/// cells up to `col_count`.
fn table_row_to_xml(row: &crate::dto::TableRowDto, col_count: usize) -> String {
    let mut writer = Writer::new(Vec::new());
    let mut tr = BytesStart::new("a:tr");
    if let Some(h) = row.height {
        tr.push_attribute(("h", h.to_string().as_str()));
    }
    writer.write_event(Event::Start(tr)).ok();
    for cell in &row.cells {
        let body: Vec<u8> = if let Some(ref tf) = cell.text_frame {
            let mut w2 = Writer::new(Vec::new());
            w2.write_event(Event::Start(BytesStart::new("a:txBody")))
                .ok();
            w2.write_event(Event::Start(BytesStart::new("a:bodyPr")))
                .ok();
            w2.write_event(Event::End(BytesEnd::new("a:bodyPr"))).ok();
            w2.write_event(Event::Empty(BytesStart::new("a:lstStyle")))
                .ok();
            for p in &tf.paragraphs {
                crate::dto::xml::write_paragraph(p, &mut w2);
            }
            w2.write_event(Event::End(BytesEnd::new("a:txBody"))).ok();
            w2.into_inner()
        } else {
            b"<a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></a:txBody>".to_vec()
        };
        writer.get_mut().write_all(b"<a:tc>").ok();
        writer.get_mut().write_all(&body).ok();
        writer.get_mut().write_all(b"</a:tc>").ok();
    }
    // Pad with empty cells up to the column count.
    for _ in row.cells.len()..col_count {
        writer
            .get_mut()
            .write_all(
                b"<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></a:txBody></a:tc>",
            )
            .ok();
    }
    writer.write_event(Event::End(BytesEnd::new("a:tr"))).ok();
    String::from_utf8(writer.into_inner()).expect("valid UTF-8")
}

/// Find the `a:tbl` element range within a shape.
fn find_table_range(events: &[Event<'_>], shape_idx: usize) -> Option<(usize, usize)> {
    let (shape_start, shape_end) = find_shape_range(events, shape_idx)?;
    find_elem_range(events, b"a:tbl", shape_start).filter(|r| r.0 <= shape_end)
}

/// Count the `a:gridCol` elements inside the table grid.
fn count_grid_cols(events: &[Event<'_>], tbl_start: usize, tbl_end: usize) -> usize {
    find_elem_range(events, b"a:tblGrid", tbl_start)
        .filter(|r| r.0 <= tbl_end)
        .map(|(s, e)| count_children(events, s, e, b"a:gridCol"))
        .unwrap_or(0)
}

/// Count direct children with the given tag name inside `[start, end]`.
fn count_children(events: &[Event<'_>], start: usize, end: usize, name: &[u8]) -> usize {
    let mut count = 0;
    let mut depth = 0u32;
    let mut i = start + 1;
    while i < end {
        match &events[i] {
            Event::Start(e) => {
                if depth == 0 && e.name().as_ref() == name {
                    count += 1;
                }
                depth += 1;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Empty(e) if depth == 0 && e.name().as_ref() == name => {
                count += 1;
            }
            _ => {}
        }
        i += 1;
    }
    count
}

/// Add a table row (`table.rows` appends, `table.rows[N]` inserts after N).
pub fn add_table_row_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value_json: &str,
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };
    let insert_after = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "rows" => Some(*i),
        [path::PathSegment::Field(n)] if n == "rows" => None,
        _ => {
            return Err(AppError::PathParse(
                "Expected table.rows or table.rows[N]".to_string(),
            ));
        }
    };

    let row: crate::dto::TableRowDto = serde_json::from_str(value_json)
        .map_err(|e| AppError::InvalidValue(format!("Invalid row JSON: {e}")))?;

    let mut events = read_events(xml_bytes)?;
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let col_count = count_grid_cols(&events, tbl_start, tbl_end);
    let new_row = table_row_to_xml(&row, col_count);
    let new_events = read_events(new_row.as_bytes())?;

    // Insert position: after the (insert_after)th a:tr end, else before tbl end.
    let insert_pos = if let Some(after) = insert_after {
        let mut count = 0usize;
        let mut pos = None;
        let mut i = tbl_start + 1;
        while i < tbl_end {
            if let Event::Start(e) = &events[i]
                && e.name().as_ref() == b"a:tr"
            {
                if count == after {
                    let (_, e) = find_elem_range(&events, b"a:tr", i).unwrap();
                    pos = Some(e + 1);
                    break;
                }
                count += 1;
            }
            i += 1;
        }
        pos.unwrap_or(tbl_end)
    } else {
        tbl_end
    };

    for ev in new_events.into_iter().rev() {
        events.insert(insert_pos, ev);
    }
    write_events(events)
}

/// Remove a table row (`table.rows[N]`).
pub fn remove_table_row_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };
    let row_idx = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "rows" => *i,
        _ => {
            return Err(AppError::PathParse("Expected table.rows[N]".to_string()));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (row_start, row_end) = find_nth_child_range(&events, tbl_start, tbl_end, b"a:tr", row_idx)
        .ok_or_else(|| AppError::PathParse(format!("Table row {row_idx} not found")))?;
    for j in (row_start..=row_end).rev() {
        events.remove(j);
    }
    write_events(events)
}

/// Add a table column (`table.grid` appends, `table.grid[N]` inserts after N).
/// Inserts a `<a:gridCol>` into the table grid and an empty `<a:tc>` into every
/// row.
pub fn add_table_column_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
    value_json: &str,
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };
    let insert_after = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "grid" => Some(*i),
        [path::PathSegment::Field(n)] if n == "grid" => None,
        _ => {
            return Err(AppError::PathParse(
                "Expected table.grid or table.grid[N]".to_string(),
            ));
        }
    };

    let col: crate::dto::GridColDto = serde_json::from_str(value_json)
        .map_err(|e| AppError::InvalidValue(format!("Invalid column JSON: {e}")))?;

    let mut events = read_events(xml_bytes)?;
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (grid_start, grid_end) = find_elem_range(&events, b"a:tblGrid", tbl_start)
        .filter(|r| r.0 <= tbl_end)
        .ok_or_else(|| AppError::PathParse("Table has no grid".to_string()))?;

    // Insert position for the new gridCol.
    let grid_insert_pos = if let Some(after) = insert_after {
        let mut count = 0usize;
        let mut pos = None;
        let mut i = grid_start + 1;
        while i < grid_end {
            if let Event::Start(e) = &events[i]
                && e.name().as_ref() == b"a:gridCol"
            {
                if count == after {
                    pos = Some(i + 1);
                    break;
                }
                count += 1;
            } else if let Event::Empty(e) = &events[i]
                && e.name().as_ref() == b"a:gridCol"
            {
                if count == after {
                    pos = Some(i + 1);
                    break;
                }
                count += 1;
            }
            i += 1;
        }
        pos.unwrap_or(grid_end)
    } else {
        grid_end
    };

    let mut grid_col = BytesStart::new("a:gridCol");
    grid_col.push_attribute(("w", col.width.to_string().as_str()));

    // Insert gridCol.
    events.insert(grid_insert_pos, Event::Empty(grid_col));

    // Insert an empty a:tc into every row.
    let empty_cell = "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></a:txBody></a:tc>".to_string();
    let cell_events = read_events(empty_cell.as_bytes())?;
    let mut i = tbl_start + 1;
    while i < tbl_end {
        if let Event::Start(e) = &events[i]
            && e.name().as_ref() == b"a:tr"
        {
            let (_, row_end) = find_elem_range(&events, b"a:tr", i).unwrap();
            for ev in cell_events.iter().rev() {
                events.insert(row_end, ev.clone());
            }
            i = row_end + 1;
        } else {
            i += 1;
        }
    }

    write_events(events)
}

/// Remove a table column (`table.grid[N]`). Removes the Nth `<a:gridCol>` and
/// the Nth `<a:tc>` from every row.
pub fn remove_table_column_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let inner = if remaining.len() > 1
        && matches!(&remaining[0], path::PathSegment::Field(n) if n == "table")
    {
        &remaining[1..]
    } else {
        remaining
    };
    let col_idx = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "grid" => *i,
        _ => {
            return Err(AppError::PathParse("Expected table.grid[N]".to_string()));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (grid_start, grid_end) = find_elem_range(&events, b"a:tblGrid", tbl_start)
        .filter(|r| r.0 <= tbl_end)
        .ok_or_else(|| AppError::PathParse("Table has no grid".to_string()))?;

    // Remove the Nth gridCol.
    let mut count = 0usize;
    let mut i = grid_start + 1;
    let mut removed_col = false;
    while i < grid_end {
        let is_col = matches!(&events[i], Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"a:gridCol");
        if is_col {
            if count == col_idx {
                events.remove(i);
                removed_col = true;
                break;
            }
            count += 1;
        }
        i += 1;
    }
    if !removed_col {
        return Err(AppError::PathParse(format!(
            "Table column {col_idx} not found"
        )));
    }

    // Removing the gridCol shifted subsequent indices; re-locate the table.
    let (tbl_start, tbl_end) =
        find_table_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;

    // Remove the Nth a:tc from every row. Collect row ranges first and process
    // in reverse so earlier indices stay stable while we remove cells.
    let mut rows = Vec::new();
    let mut i = tbl_start + 1;
    while i < tbl_end {
        if let Event::Start(e) = &events[i]
            && e.name().as_ref() == b"a:tr"
        {
            let (row_start, row_end) = find_elem_range(&events, b"a:tr", i).unwrap();
            rows.push((row_start, row_end));
            i = row_end + 1;
        } else {
            i += 1;
        }
    }
    for (row_start, row_end) in rows.into_iter().rev() {
        let (cell_start, cell_end) = find_nth_child_range(
            &events, row_start, row_end, b"a:tc", col_idx,
        )
        .ok_or_else(|| AppError::PathParse(format!("Table cell {col_idx} not found in row")))?;
        for j in (cell_start..=cell_end).rev() {
            events.remove(j);
        }
    }

    write_events(events)
}

pub(crate) fn is_chart_type_tag(name: &[u8]) -> bool {
    matches!(
        name,
        b"c:barChart"
            | b"c:lineChart"
            | b"c:pieChart"
            | b"c:scatterChart"
            | b"c:doughnutChart"
            | b"c:radarChart"
            | b"c:areaChart"
    )
}

fn replace_text_in_range(events: &mut Vec<Event<'static>>, start: usize, end: usize, value: &str) {
    let mut to_remove: Vec<usize> = Vec::new();
    for (j, ev) in events.iter().enumerate().take(end + 1).skip(start) {
        if matches!(ev, Event::Text(_) | Event::GeneralRef(_)) {
            to_remove.push(j);
        }
    }
    for j in to_remove.into_iter().rev() {
        events.remove(j);
    }
    events.insert(
        start + 1,
        Event::Text(BytesText::from_escaped(escape(value.to_string()))),
    );
}

fn replace_pt_text(
    events: &mut Vec<Event<'static>>,
    container_start: usize,
    container_end: usize,
    pt_idx: usize,
    value: &str,
) -> AppResult<()> {
    let cache_names: [&[u8]; 4] = [b"c:strCache", b"c:numCache", b"c:strLit", b"c:numLit"];
    let cache_range = cache_names
        .iter()
        .find_map(|n| find_elem_range(events, n, container_start).filter(|r| r.0 <= container_end))
        .ok_or_else(|| AppError::PathParse("Chart cache not found".to_string()))?;

    let (pt_start, pt_end) =
        find_nth_child_range(events, cache_range.0, cache_range.1, b"c:pt", pt_idx)
            .ok_or_else(|| AppError::PathParse(format!("Chart point {pt_idx} not found")))?;

    let (v_start, v_end) = find_elem_range(events, b"c:v", pt_start)
        .filter(|r| r.0 <= pt_end)
        .ok_or_else(|| AppError::PathParse("Chart value element not found".to_string()))?;

    replace_text_in_range(events, v_start, v_end, value);
    Ok(())
}

pub fn replace_chart_property_lossless(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    if inner.is_empty() {
        return Err(AppError::PathParse("Empty chart path".to_string()));
    }

    let mut events = read_events(xml_bytes)?;

    let chart_range = find_elem_range(&events, b"c:chart", 0)
        .ok_or_else(|| AppError::PathParse("Chart element not found".to_string()))?;
    let plot_range = find_elem_range(&events, b"c:plotArea", chart_range.0)
        .filter(|r| r.0 <= chart_range.1)
        .ok_or_else(|| AppError::PathParse("Chart plotArea not found".to_string()))?;

    match &inner[0] {
        path::PathSegment::Field(name) if name == "chart_type" => {
            let ct_start = events[plot_range.0..=plot_range.1]
                .iter()
                .enumerate()
                .find_map(|(i, ev)| match ev {
                    Event::Start(e) if is_chart_type_tag(e.name().as_ref()) => {
                        Some(plot_range.0 + i)
                    }
                    _ => None,
                })
                .ok_or_else(|| AppError::PathParse("Chart type element not found".to_string()))?;
            let ct_name = match &events[ct_start] {
                Event::Start(e) => e.name().as_ref().to_vec(),
                _ => unreachable!(),
            };
            let (ct_start, ct_end) = find_elem_range(&events, &ct_name, ct_start).unwrap();

            let new_name = if value.starts_with("c:") {
                value.to_string()
            } else {
                format!("c:{value}")
            };
            events[ct_start] = Event::Start(BytesStart::new(new_name.clone()));
            events[ct_end] = Event::End(BytesEnd::new(new_name));
        }
        path::PathSegment::Field(name) if name == "series" => {
            let ser_idx = match inner.get(1) {
                Some(path::PathSegment::Index(i)) => *i,
                _ => return Err(AppError::PathParse("Expected series index".to_string())),
            };
            let prop = match inner.get(2) {
                Some(path::PathSegment::Field(n)) => n.as_str(),
                _ => {
                    return Err(AppError::PathParse(
                        "Expected series property (name/categories/values)".to_string(),
                    ));
                }
            };
            let pt_idx = match inner.get(3) {
                Some(path::PathSegment::Index(i)) => Some(*i),
                _ => None,
            };

            let (ser_start, ser_end) =
                find_nth_elem_range(&events, b"c:ser", plot_range.0, plot_range.1, ser_idx)
                    .ok_or_else(|| {
                        AppError::PathParse(format!("Series index {ser_idx} not found"))
                    })?;

            match prop {
                "name" => {
                    if pt_idx.is_some() {
                        return Err(AppError::PathParse(
                            "Series name takes no index".to_string(),
                        ));
                    }
                    let tx_range = find_elem_range(&events, b"c:tx", ser_start)
                        .filter(|r| r.0 <= ser_end)
                        .ok_or_else(|| AppError::PathParse("Series has no name".to_string()))?;
                    replace_pt_text(&mut events, tx_range.0, tx_range.1, 0, value)?;
                }
                "categories" | "values" => {
                    let pt_idx = pt_idx.ok_or_else(|| {
                        AppError::PathParse(format!("{prop} point index required"))
                    })?;
                    let target_name: &[u8] = if prop == "categories" {
                        b"c:cat"
                    } else {
                        b"c:val"
                    };
                    let target_range = find_elem_range(&events, target_name, ser_start)
                        .filter(|r| r.0 <= ser_end)
                        .ok_or_else(|| AppError::PathParse(format!("Series has no {prop}")))?;
                    replace_pt_text(&mut events, target_range.0, target_range.1, pt_idx, value)?;
                }
                _ => {
                    return Err(AppError::PathParse(format!(
                        "Unknown series property '{prop}'"
                    )));
                }
            }
        }
        _ => {
            return Err(AppError::PathParse(format!(
                "Unknown chart property '{}'",
                match &inner[0] {
                    path::PathSegment::Field(n) => n.as_str(),
                    _ => "?",
                }
            )));
        }
    }

    write_events(events)
}

/// Build the XML events for a new `<c:ser>` element from a chart series DTO.
fn chart_series_events(series: &crate::dto::ChartSeriesDto, idx: usize) -> Vec<Event<'static>> {
    let mut out = Vec::new();
    out.push(Event::Start(BytesStart::new("c:ser")));
    let mut idx_el = BytesStart::new("c:idx");
    idx_el.push_attribute(("val", idx.to_string().as_str()));
    out.push(Event::Empty(idx_el));
    let mut order_el = BytesStart::new("c:order");
    order_el.push_attribute(("val", idx.to_string().as_str()));
    out.push(Event::Empty(order_el));

    if let Some(name) = &series.name {
        out.push(Event::Start(BytesStart::new("c:tx")));
        out.push(Event::Start(BytesStart::new("c:strRef")));
        out.push(Event::Start(BytesStart::new("c:strCache")));
        let mut pt_count = BytesStart::new("c:ptCount");
        pt_count.push_attribute(("val", "1"));
        out.push(Event::Empty(pt_count));
        out.push(Event::Start(BytesStart::new("c:pt")));
        out.push(Event::Start(BytesStart::new("c:v")));
        out.push(Event::Text(BytesText::from_escaped(escape(name.clone()))));
        out.push(Event::End(BytesEnd::new("c:v")));
        out.push(Event::End(BytesEnd::new("c:pt")));
        out.push(Event::End(BytesEnd::new("c:strCache")));
        out.push(Event::End(BytesEnd::new("c:strRef")));
        out.push(Event::End(BytesEnd::new("c:tx")));
    }

    out.push(Event::Start(BytesStart::new("c:cat")));
    out.push(Event::Start(BytesStart::new("c:strRef")));
    out.push(Event::Start(BytesStart::new("c:strCache")));
    let mut pt_count = BytesStart::new("c:ptCount");
    pt_count.push_attribute(("val", series.categories.len().to_string().as_str()));
    out.push(Event::Empty(pt_count));
    for (j, cat) in series.categories.iter().enumerate() {
        let mut pt = BytesStart::new("c:pt");
        pt.push_attribute(("idx", j.to_string().as_str()));
        out.push(Event::Start(pt));
        out.push(Event::Start(BytesStart::new("c:v")));
        out.push(Event::Text(BytesText::from_escaped(escape(cat.clone()))));
        out.push(Event::End(BytesEnd::new("c:v")));
        out.push(Event::End(BytesEnd::new("c:pt")));
    }
    out.push(Event::End(BytesEnd::new("c:strCache")));
    out.push(Event::End(BytesEnd::new("c:strRef")));
    out.push(Event::End(BytesEnd::new("c:cat")));

    out.push(Event::Start(BytesStart::new("c:val")));
    out.push(Event::Start(BytesStart::new("c:numRef")));
    out.push(Event::Start(BytesStart::new("c:numCache")));
    let format = BytesStart::new("c:formatCode");
    out.push(Event::Start(format));
    out.push(Event::Text(BytesText::from_escaped(escape(
        "General".to_string(),
    ))));
    out.push(Event::End(BytesEnd::new("c:formatCode")));
    let mut pt_count = BytesStart::new("c:ptCount");
    pt_count.push_attribute(("val", series.values.len().to_string().as_str()));
    out.push(Event::Empty(pt_count));
    for (j, val) in series.values.iter().enumerate() {
        let mut pt = BytesStart::new("c:pt");
        pt.push_attribute(("idx", j.to_string().as_str()));
        out.push(Event::Start(pt));
        out.push(Event::Start(BytesStart::new("c:v")));
        out.push(Event::Text(BytesText::from_escaped(escape(
            val.to_string(),
        ))));
        out.push(Event::End(BytesEnd::new("c:v")));
        out.push(Event::End(BytesEnd::new("c:pt")));
    }
    out.push(Event::End(BytesEnd::new("c:numCache")));
    out.push(Event::End(BytesEnd::new("c:numRef")));
    out.push(Event::End(BytesEnd::new("c:val")));

    out.push(Event::End(BytesEnd::new("c:ser")));
    out
}

/// Find the range of the chart-type element (e.g. `c:barChart`) inside plotArea.
fn find_chart_type_range(events: &[Event<'_>]) -> Option<(usize, usize)> {
    let chart_range = find_elem_range(events, b"c:chart", 0)?;
    let plot_range =
        find_elem_range(events, b"c:plotArea", chart_range.0).filter(|r| r.0 <= chart_range.1)?;
    let mut i = plot_range.0 + 1;
    while i < plot_range.1 {
        if let Event::Start(e) = &events[i]
            && is_chart_type_tag(e.name().as_ref())
        {
            return find_elem_range(events, e.name().as_ref(), i).filter(|r| r.0 <= plot_range.1);
        }
        i += 1;
    }
    None
}

/// Count the `c:ser` elements inside the chart-type element.
fn count_series(events: &[Event<'_>], ct_start: usize, ct_end: usize) -> usize {
    let mut count = 0;
    let mut i = ct_start + 1;
    while i < ct_end {
        if let Event::Start(e) = &events[i]
            && e.name().as_ref() == b"c:ser"
        {
            count += 1;
        }
        i += 1;
    }
    count
}

/// Add a chart series (`chart.series` appends, `chart.series[N]` inserts after N).
pub fn add_chart_series_lossless(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
    value_json: &str,
) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    if !matches!(inner, [path::PathSegment::Field(n), ..] if n == "series") {
        return Err(AppError::PathParse(
            "Expected chart.series or chart.series[N]".to_string(),
        ));
    }
    let insert_after = match inner.get(1) {
        Some(path::PathSegment::Index(i)) => Some(*i),
        Some(path::PathSegment::Field(_)) | None => None,
    };

    let series: crate::dto::ChartSeriesDto = serde_json::from_str(value_json)
        .map_err(|e| AppError::InvalidValue(format!("Invalid series JSON: {e}")))?;

    let mut events = read_events(xml_bytes)?;
    let (ct_start, ct_end) = find_chart_type_range(&events)
        .ok_or_else(|| AppError::PathParse("Chart type element not found".to_string()))?;
    let next_idx = count_series(&events, ct_start, ct_end);
    let new_events = chart_series_events(&series, next_idx);

    // Insert position: after the (insert_after)th c:ser end, else before ct_end.
    let insert_pos = if let Some(after) = insert_after {
        let mut count = 0usize;
        let mut pos = None;
        let mut i = ct_start + 1;
        while i < ct_end {
            if let Event::Start(e) = &events[i]
                && e.name().as_ref() == b"c:ser"
            {
                if count == after {
                    let (_, e) = find_elem_range(&events, b"c:ser", i).unwrap();
                    pos = Some(e + 1);
                    break;
                }
                count += 1;
            }
            i += 1;
        }
        pos.unwrap_or(ct_end)
    } else {
        ct_end
    };

    for ev in new_events.into_iter().rev() {
        events.insert(insert_pos, ev);
    }
    write_events(events)
}

/// Remove a chart series (`chart.series[N]`).
pub fn remove_chart_series_lossless(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
) -> AppResult<Vec<u8>> {
    let inner = if matches!(&remaining[0], path::PathSegment::Field(n) if n == "chart") {
        &remaining[1..]
    } else {
        remaining
    };
    let ser_idx = match inner {
        [path::PathSegment::Field(n), path::PathSegment::Index(i)] if n == "series" => *i,
        _ => {
            return Err(AppError::PathParse("Expected chart.series[N]".to_string()));
        }
    };

    let mut events = read_events(xml_bytes)?;
    let (ct_start, ct_end) = find_chart_type_range(&events)
        .ok_or_else(|| AppError::PathParse("Chart type element not found".to_string()))?;
    let (ser_start, ser_end) = find_nth_elem_range(&events, b"c:ser", ct_start, ct_end, ser_idx)
        .ok_or_else(|| AppError::PathParse(format!("Series {ser_idx} not found")))?;
    for j in (ser_start..=ser_end).rev() {
        events.remove(j);
    }
    write_events(events)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn set_rpr_attr(
    events: &mut Vec<Event<'static>>,
    rpr_range: &Option<(usize, usize)>,
    insert_pos: usize,
    key: &[u8],
    val: &str,
) {
    if let Some((s, e)) = *rpr_range {
        if s == e {
            if let Event::Empty(ref orig) = events[s] {
                events[s] = Event::Empty(copy_attrs(orig, key, val));
            }
        } else if let Event::Start(ref orig) = events[s] {
            events[s] = Event::Start(copy_attrs(orig, key, val));
        }
    } else {
        let mut new_rpr = BytesStart::new("a:rPr");
        let key_str = String::from_utf8_lossy(key).to_string();
        new_rpr.push_attribute((key_str.as_str(), val));
        events.insert(insert_pos, Event::End(BytesEnd::new("a:rPr")));
        events.insert(insert_pos, Event::Start(new_rpr));
    }
}

fn set_rpr_latin(
    events: &mut Vec<Event<'static>>,
    rpr_range: &Option<(usize, usize)>,
    insert_pos: usize,
    value: &str,
) {
    if let Some((s, e)) = *rpr_range {
        let latin_range = find_elem_range(events, b"a:latin", s).filter(|r| r.0 <= e);
        if let Some((ls, _le)) = latin_range {
            if let Event::Empty(ref orig) = events[ls] {
                events[ls] = Event::Empty(copy_attrs(orig, b"typeface", value));
            } else if let Event::Start(ref orig) = events[ls] {
                events[ls] = Event::Start(copy_attrs(orig, b"typeface", value));
            }
        } else {
            let insert_at = expand_empty_container(events, s, b"a:rPr");
            let mut latin = BytesStart::new("a:latin");
            latin.push_attribute(("typeface", value));
            events.insert(insert_at, Event::Empty(latin));
        }
    } else {
        let rpr = BytesStart::new("a:rPr");
        let mut latin = BytesStart::new("a:latin");
        latin.push_attribute(("typeface", value));
        events.insert(insert_pos, Event::End(BytesEnd::new("a:rPr")));
        events.insert(insert_pos, Event::Empty(latin));
        events.insert(insert_pos, Event::Start(rpr));
    }
}

fn color_xml_events(value: &str) -> Vec<Event<'static>> {
    let mut result = Vec::new();
    result.push(Event::Start(BytesStart::new("a:solidFill")));
    if let Some(stripped) = value.strip_prefix('#') {
        let mut clr = BytesStart::new("a:srgbClr");
        clr.push_attribute(("val", stripped));
        result.push(Event::Empty(clr));
    } else if value.len() == 6 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut clr = BytesStart::new("a:srgbClr");
        clr.push_attribute(("val", value));
        result.push(Event::Empty(clr));
    } else {
        let mut clr = BytesStart::new("a:schemeClr");
        clr.push_attribute(("val", value));
        result.push(Event::Empty(clr));
    }
    result.push(Event::End(BytesEnd::new("a:solidFill")));
    result
}

fn set_rpr_color(
    events: &mut Vec<Event<'static>>,
    rpr_range: &Option<(usize, usize)>,
    insert_pos: usize,
    value: &str,
) {
    if let Some((s, e)) = *rpr_range {
        remove_solid_fill_children(events, s, e);
        let insert_at = expand_empty_container(events, s, b"a:rPr");
        let color_events = color_xml_events(value);
        for (j, ev) in color_events.into_iter().enumerate() {
            events.insert(insert_at + j, ev);
        }
    } else {
        let rpr = BytesStart::new("a:rPr");
        events.insert(insert_pos, Event::End(BytesEnd::new("a:rPr")));
        let color_events = color_xml_events(value);
        for ev in color_events.into_iter().rev() {
            events.insert(insert_pos, ev);
        }
        events.insert(insert_pos, Event::Start(rpr));
    }
}

fn set_end_rpr_attr(
    events: &mut Vec<Event<'static>>,
    rpr_range: &Option<(usize, usize)>,
    insert_pos: usize,
    key: &[u8],
    val: &str,
) {
    let elem_name = "a:endParaRPr";
    if let Some((s, e)) = *rpr_range {
        if s == e {
            if let Event::Empty(ref orig) = events[s] {
                events[s] = Event::Empty(copy_attrs(orig, key, val));
            }
        } else if let Event::Start(ref orig) = events[s] {
            events[s] = Event::Start(copy_attrs(orig, key, val));
        }
    } else {
        let mut new_rpr = BytesStart::new(elem_name);
        let key_str = String::from_utf8_lossy(key).to_string();
        new_rpr.push_attribute((key_str.as_str(), val));
        events.insert(insert_pos, Event::End(BytesEnd::new(elem_name)));
        events.insert(insert_pos, Event::Start(new_rpr));
    }
}

fn set_end_rpr_latin(
    events: &mut Vec<Event<'static>>,
    rpr_range: &Option<(usize, usize)>,
    insert_pos: usize,
    value: &str,
) {
    let elem_name = "a:endParaRPr";
    if let Some((s, e)) = *rpr_range {
        let latin_range = find_elem_range(events, b"a:latin", s).filter(|r| r.0 <= e);
        if let Some((ls, _le)) = latin_range {
            if let Event::Empty(ref orig) = events[ls] {
                events[ls] = Event::Empty(copy_attrs(orig, b"typeface", value));
            } else if let Event::Start(ref orig) = events[ls] {
                events[ls] = Event::Start(copy_attrs(orig, b"typeface", value));
            }
        } else {
            let insert_at = expand_empty_container(events, s, b"a:endParaRPr");
            let mut latin = BytesStart::new("a:latin");
            latin.push_attribute(("typeface", value));
            events.insert(insert_at, Event::Empty(latin));
        }
    } else {
        let rpr = BytesStart::new(elem_name);
        let mut latin = BytesStart::new("a:latin");
        latin.push_attribute(("typeface", value));
        events.insert(insert_pos, Event::End(BytesEnd::new(elem_name)));
        events.insert(insert_pos, Event::Empty(latin));
        events.insert(insert_pos, Event::Start(rpr));
    }
}

fn set_end_rpr_color(
    events: &mut Vec<Event<'static>>,
    rpr_range: &Option<(usize, usize)>,
    insert_pos: usize,
    value: &str,
) {
    let elem_name = "a:endParaRPr";
    if let Some((s, e)) = *rpr_range {
        remove_solid_fill_children(events, s, e);
        let insert_at = expand_empty_container(events, s, b"a:endParaRPr");
        let color_events = color_xml_events(value);
        for (j, ev) in color_events.into_iter().enumerate() {
            events.insert(insert_at + j, ev);
        }
    } else {
        let rpr = BytesStart::new(elem_name);
        events.insert(insert_pos, Event::End(BytesEnd::new(elem_name)));
        let color_events = color_xml_events(value);
        for ev in color_events.into_iter().rev() {
            events.insert(insert_pos, ev);
        }
        events.insert(insert_pos, Event::Start(rpr));
    }
}

fn set_ppr_attr(
    events: &mut Vec<Event<'static>>,
    ppr_range: &Option<(usize, usize)>,
    insert_pos: usize,
    key: &[u8],
    val: &str,
) {
    let range = *ppr_range;
    if let Some((s, e)) = range {
        if s == e {
            if let Event::Empty(ref orig) = events[s] {
                events[s] = Event::Empty(copy_attrs(orig, key, val));
            }
        } else if let Event::Start(ref orig) = events[s] {
            events[s] = Event::Start(copy_attrs(orig, key, val));
        }
    } else {
        let mut new_ppr = BytesStart::new("a:pPr");
        let key_str = String::from_utf8_lossy(key).to_string();
        new_ppr.push_attribute((key_str.as_str(), val));
        events.insert(insert_pos, Event::End(BytesEnd::new("a:pPr")));
        events.insert(insert_pos, Event::Start(new_ppr));
    }
}

fn edit_spacing_elem(
    events: &mut Vec<Event<'static>>,
    para_start: usize,
    para_end: usize,
    spacing_tag: &[u8],
    value: &str,
    is_line_spacing: bool,
) {
    let (ppr_range, ppr_insert) = find_ppr_in_para(events, para_start, para_end);

    let ppr_start = if let Some((s, _e)) = ppr_range {
        s
    } else {
        let ppr = BytesStart::new("a:pPr");
        events.insert(ppr_insert, Event::End(BytesEnd::new("a:pPr")));
        events.insert(ppr_insert, Event::Start(ppr));
        ppr_insert
    };

    let ppr_end = if let Some((_s, e)) = ppr_range {
        e
    } else {
        ppr_insert + 1
    };

    let spacing_range = find_elem_range(events, spacing_tag, ppr_start).filter(|r| r.0 <= ppr_end);

    let spc_start = if let Some((s, _e)) = spacing_range {
        s
    } else {
        let spc_name = String::from_utf8_lossy(spacing_tag).to_string();
        let spc_name_end = spc_name.clone();
        events.insert(ppr_start + 1, Event::End(BytesEnd::new(spc_name_end)));
        events.insert(ppr_start + 1, Event::Start(BytesStart::new(spc_name)));
        ppr_start + 1
    };

    let spc_end = if let Some((_s, e)) = spacing_range {
        e
    } else {
        spc_start + 1
    };

    let val_tag = if is_line_spacing {
        match value.parse::<f64>() {
            Ok(v) if (0.5..=10.0).contains(&v) => b"a:spcPct",
            _ => b"a:spcPts",
        }
    } else {
        b"a:spcPts"
    };

    let val_range = find_elem_range(events, val_tag, spc_start).filter(|r| r.0 <= spc_end);

    let formatted = if is_line_spacing {
        if val_tag == b"a:spcPct" {
            let v: f64 = value.parse().unwrap_or(0.0);
            (v * 100000.0).round().to_string()
        } else {
            let v: f64 = value.parse().unwrap_or(0.0);
            (v * 100.0).round().to_string()
        }
    } else {
        value.to_string()
    };

    if let Some((vs, _ve)) = val_range {
        if let Event::Empty(ref orig) = events[vs] {
            events[vs] = Event::Empty(copy_attrs(orig, b"val", &formatted));
        }
    } else {
        let val_tag_owned = String::from_utf8_lossy(val_tag).to_string();
        let mut val_elem = BytesStart::new(val_tag_owned);
        val_elem.push_attribute(("val", formatted.as_str()));
        events.insert(spc_start + 1, Event::Empty(val_elem));
    }
}

fn remove_solid_fill_children(
    events: &mut Vec<Event<'static>>,
    parent_start: usize,
    parent_end: usize,
) {
    let mut to_remove: Vec<usize> = Vec::new();
    let mut i = parent_start + 1;
    while i < parent_end {
        if let Event::Start(e) = &events[i]
            && e.name().as_ref() == b"a:solidFill"
            && let Some(range) = find_elem_range(events, b"a:solidFill", i)
        {
            for j in (range.0..=range.1).rev() {
                to_remove.push(j);
            }
            i = range.1 + 1;
            continue;
        }
        if let Event::Empty(e) = &events[i]
            && e.name().as_ref() == b"a:solidFill"
        {
            to_remove.push(i);
        }
        i += 1;
    }
    to_remove.sort_unstable();
    to_remove.dedup();
    to_remove.reverse();
    for idx in to_remove {
        events.remove(idx);
    }
}

/// Map a snake_case core-property name to its element tag.
fn core_prop_tag(name: &str) -> Option<&'static str> {
    match name {
        "title" => Some("dc:title"),
        "subject" => Some("dc:subject"),
        "author" => Some("dc:creator"),
        "keywords" => Some("cp:keywords"),
        "comments" => Some("dc:description"),
        "last_modified_by" => Some("cp:lastModifiedBy"),
        "revision" => Some("cp:revision"),
        "created" => Some("dcterms:created"),
        "modified" => Some("dcterms:modified"),
        "category" => Some("cp:category"),
        "content_status" => Some("cp:contentStatus"),
        _ => None,
    }
}

/// Map an element tag to its snake_case core-property name.
pub fn core_prop_key(tag: &str) -> Option<&'static str> {
    match tag {
        "dc:title" => Some("title"),
        "dc:subject" => Some("subject"),
        "dc:creator" => Some("author"),
        "cp:keywords" => Some("keywords"),
        "dc:description" => Some("comments"),
        "cp:lastModifiedBy" => Some("last_modified_by"),
        "cp:revision" => Some("revision"),
        "dcterms:created" => Some("created"),
        "dcterms:modified" => Some("modified"),
        "cp:category" => Some("category"),
        "cp:contentStatus" => Some("content_status"),
        _ => None,
    }
}

/// Set the text of a core property in core.xml, creating the element if it
/// is missing. The value is escaped for XML text content.
pub fn replace_core_property(xml_bytes: &[u8], name: &str, value: &str) -> AppResult<Vec<u8>> {
    let tag = core_prop_tag(name)
        .ok_or_else(|| AppError::PathParse(format!("Unknown core property '{name}'")))?;

    let mut events = read_events(xml_bytes)?;

    // Find the cp:coreProperties root and its closing element.
    let mut root_start: Option<usize> = None;
    for (i, ev) in events.iter().enumerate() {
        if let Event::Start(e) = ev
            && e.name().as_ref() == b"cp:coreProperties"
        {
            root_start = Some(i);
            break;
        }
    }
    let root_start =
        root_start.ok_or_else(|| AppError::PathParse("No cp:coreProperties root".to_string()))?;

    let mut root_end: Option<usize> = None;
    {
        let mut depth = 0u32;
        for (j, ev2) in events.iter().enumerate().skip(root_start) {
            match ev2 {
                Event::Start(_) => depth += 1,
                Event::End(e2) => {
                    depth = depth.saturating_sub(1);
                    if e2.name().as_ref() == b"cp:coreProperties" && depth == 0 {
                        root_end = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    let root_end = root_end
        .ok_or_else(|| AppError::PathParse("No cp:coreProperties root found".to_string()))?;

    // Locate the property element (self-closing Empty, or Start..End).
    let mut prop: Option<(usize, usize, bool)> = None;
    {
        let mut depth = 0u32;
        let mut start: Option<usize> = None;
        for j in (root_start + 1)..root_end {
            match &events[j] {
                Event::Start(e) => {
                    if depth == 0 && e.name().as_ref() == tag.as_bytes() {
                        start = Some(j);
                    }
                    depth += 1;
                }
                Event::Empty(e) if depth == 0 && e.name().as_ref() == tag.as_bytes() => {
                    prop = Some((j, j, true));
                    break;
                }
                Event::End(e) => {
                    if let (Some(s), _) = (start, events.get(j))
                        && depth == 1
                        && e.name().as_ref() == tag.as_bytes()
                    {
                        prop = Some((s, j, false));
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
    }

    let text_event = |value: &str| BytesText::from_escaped(escape(value.to_string()));

    match prop {
        Some((start, _end, true)) => {
            // Self-closing element -> expand to start + text + end.
            let Event::Empty(e) = events.remove(start) else {
                unreachable!()
            };
            let name_str = String::from_utf8_lossy(e.name().as_ref()).to_string();
            let mut start_elem = BytesStart::new(name_str.clone());
            for a in e.attributes().flatten() {
                start_elem.push_attribute((a.key.as_ref(), a.value.as_ref()));
            }
            events.insert(start, Event::End(BytesEnd::new(name_str)));
            events.insert(start, Event::Text(text_event(value)));
            events.insert(start, Event::Start(start_elem));
        }
        Some((start, end, false)) => {
            // Replace the text content directly inside the element, inserting
            // it if the element is currently empty.
            replace_text_in_range(&mut events, start, end, value);
        }
        None => {
            // Insert a new element just before the root's closing tag.
            events.insert(root_end, Event::End(BytesEnd::new(tag.to_string())));
            events.insert(root_end, Event::Text(text_event(value)));
            events.insert(root_end, Event::Start(BytesStart::new(tag.to_string())));
        }
    }

    write_events(events)
}

const BG_FILL_TAGS: [&[u8]; 7] = [
    b"a:solidFill",
    b"a:noFill",
    b"a:gradFill",
    b"a:blipFill",
    b"a:pattFill",
    b"a:grpFill",
    b"a:bgFill",
];

fn remove_children_by_name(
    events: &mut Vec<Event<'static>>,
    parent_start: usize,
    parent_end: usize,
    names: &[&[u8]],
) {
    let mut to_remove: Vec<usize> = Vec::new();
    let mut i = parent_start + 1;
    while i < parent_end {
        if let Event::Start(e) = &events[i]
            && names.contains(&e.name().as_ref())
            && let Some(range) = find_elem_range(events, e.name().as_ref(), i)
        {
            for j in (range.0..=range.1).rev() {
                to_remove.push(j);
            }
            i = range.1 + 1;
            continue;
        } else if let Event::Empty(e) = &events[i]
            && names.contains(&e.name().as_ref())
        {
            to_remove.push(i);
        }
        i += 1;
    }
    to_remove.sort_unstable();
    to_remove.dedup();
    for j in to_remove.into_iter().rev() {
        events.remove(j);
    }
}

pub fn set_slide_background(xml_bytes: &[u8], color: &str) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;

    let csld_start = events
        .iter()
        .position(|e| matches!(e, Event::Start(ev) if ev.name().as_ref() == b"p:cSld"))
        .ok_or_else(|| AppError::PathParse("No p:cSld found in slide".to_string()))?;

    let fill_events = color_xml_events(color);

    let bg_range = find_elem_range(&events, b"p:bg", csld_start);

    if let Some((bs, be)) = bg_range {
        let bgpr = find_elem_range(&events, b"p:bgPr", bs).filter(|r| r.0 <= be);
        match bgpr {
            Some((ps, pe)) => {
                remove_children_by_name(&mut events, ps, pe, &BG_FILL_TAGS);
                for (j, ev) in fill_events.into_iter().enumerate() {
                    events.insert(ps + 1 + j, ev);
                }
            }
            None => {
                remove_children_by_name(&mut events, bs, be, &[b"p:bgRef"]);
                events.insert(bs + 1, Event::End(BytesEnd::new("p:bgPr")));
                for ev in fill_events.into_iter().rev() {
                    events.insert(bs + 1, ev);
                }
                events.insert(bs + 1, Event::Start(BytesStart::new("p:bgPr")));
            }
        }
    } else {
        events.insert(csld_start + 1, Event::End(BytesEnd::new("p:bg")));
        events.insert(csld_start + 1, Event::End(BytesEnd::new("p:bgPr")));
        for ev in fill_events.into_iter().rev() {
            events.insert(csld_start + 1, ev);
        }
        events.insert(csld_start + 1, Event::Start(BytesStart::new("p:bgPr")));
        events.insert(csld_start + 1, Event::Start(BytesStart::new("p:bg")));
    }

    write_events(events)
}

pub fn parse_slide_background(xml_bytes: &[u8]) -> serde_json::Value {
    let events = match read_events(xml_bytes) {
        Ok(e) => e,
        Err(_) => return serde_json::Value::Null,
    };
    let csld_start = match events
        .iter()
        .position(|e| matches!(e, Event::Start(ev) if ev.name().as_ref() == b"p:cSld"))
    {
        Some(i) => i,
        None => return serde_json::Value::Null,
    };
    let Some((bs, be)) = find_elem_range(&events, b"p:bg", csld_start) else {
        return json!({ "fill": { "type": serde_json::Value::Null } });
    };
    if let Some((ps, pe)) = find_elem_range(&events, b"p:bgPr", bs).filter(|r| r.0 <= be) {
        if let Some((fs, fe)) = find_elem_range(&events, b"a:solidFill", ps).filter(|r| r.0 <= pe) {
            let color = (fs..=fe)
                .find(|&i| {
                    matches!(&events[i], Event::Empty(e) | Event::Start(e) if e.name().as_ref() == b"a:srgbClr")
                })
                .and_then(|i| {
                    let (Event::Empty(e) | Event::Start(e)) = &events[i] else {
                        return None;
                    };
                    e.attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"val")
                        .map(|a| String::from_utf8_lossy(&a.value).to_string())
                });
            return json!({
                "fill": {
                    "type": "SOLID",
                    "color": color.unwrap_or_default(),
                }
            });
        }
        return json!({ "fill": { "type": "GRADIENT" } });
    }
    if find_elem_range(&events, b"p:bgRef", bs).is_some_and(|r| r.0 <= be) {
        return json!({ "fill": { "type": "THEME" } });
    }
    json!({ "fill": { "type": serde_json::Value::Null } })
}

pub const THEME_COLOR_NAMES: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

fn set_srgb_attr(events: &mut [Event<'static>], s: usize, e: usize, value: &str) {
    if s == e {
        if let Event::Empty(orig) = &events[s] {
            events[s] = Event::Empty(copy_attrs(orig, b"val", value));
        }
    } else if let Event::Start(orig) = &events[s] {
        events[s] = Event::Start(copy_attrs(orig, b"val", value));
    }
}

fn set_latin_typeface(events: &mut [Event<'static>], s: usize, e: usize, value: &str) {
    if s == e {
        if let Event::Empty(orig) = &events[s] {
            events[s] = Event::Empty(copy_attrs(orig, b"typeface", value));
        }
    } else if let Event::Start(orig) = &events[s] {
        events[s] = Event::Start(copy_attrs(orig, b"typeface", value));
    }
}

/// Replace a theme color (`p.theme.colors.accent1`) or font
/// (`p.theme.fonts.major` / `.minor`). `remaining` is the segments after
/// `theme`.
pub fn replace_theme_property(
    xml_bytes: &[u8],
    remaining: &[path::PathSegment],
    value: &str,
) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;

    match remaining {
        [path::PathSegment::Field(n), path::PathSegment::Field(prop)]
            if n == "colors" && THEME_COLOR_NAMES.contains(&prop.as_str()) =>
        {
            let clr_range = find_elem_range(&events, b"a:clrScheme", 0)
                .ok_or_else(|| AppError::PathParse("No color scheme in theme".to_string()))?;
            let child_name = format!("a:{prop}");
            let child =
                find_child_elem_range(&events, clr_range.0, clr_range.1, child_name.as_bytes())
                    .ok_or_else(|| {
                        AppError::PathParse(format!("Theme color '{prop}' not found"))
                    })?;
            let srgb = find_child_elem_range(&events, child.0, child.1, b"a:srgbClr");
            match srgb {
                Some((s, e)) => set_srgb_attr(&mut events, s, e, value),
                None => {
                    let mut clr = BytesStart::new("a:srgbClr");
                    clr.push_attribute(("val", value));
                    events.insert(child.0 + 1, Event::Empty(clr));
                }
            }
        }
        [path::PathSegment::Field(n), path::PathSegment::Field(prop)]
            if n == "fonts" && matches!(prop.as_str(), "major" | "minor") =>
        {
            let font_range = find_elem_range(&events, b"a:fontScheme", 0)
                .ok_or_else(|| AppError::PathParse("No font scheme in theme".to_string()))?;
            let family = if prop == "major" {
                b"a:majorFont"
            } else {
                b"a:minorFont"
            };
            let family_range =
                find_child_elem_range(&events, font_range.0, font_range.1, family)
                    .ok_or_else(|| AppError::PathParse("Font family not found".to_string()))?;
            let latin = find_child_elem_range(&events, family_range.0, family_range.1, b"a:latin");
            match latin {
                Some((s, e)) => set_latin_typeface(&mut events, s, e, value),
                None => {
                    let mut latin = BytesStart::new("a:latin");
                    latin.push_attribute(("typeface", value));
                    events.insert(family_range.0 + 1, Event::Empty(latin));
                }
            }
        }
        _ => {
            return Err(AppError::PathParse(
                "Expected theme.colors.<name> or theme.fonts.major/minor".to_string(),
            ));
        }
    }

    write_events(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLIDE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
       xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
       xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
      <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
      <p:sp>
        <p:nvSpPr><p:cNvPr id="2" name="S1"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
        <p:spPr>
          <a:xfrm><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm>
          <a:gradFill><a:gsLst><a:gs pos="0"><a:srgbClr val="FF0000"/></a:gs></a:gsLst></a:gradFill>
        </p:spPr>
        <p:txBody>
          <a:bodyPr wrap="square"/>
          <a:p><a:r><a:rPr lang="en-US" sz="1200"/><a:t>Hello</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
      <p:graphicFrame>
        <p:nvGraphicFramePr><p:cNvPr id="3" name="T1"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>
        <p:xfrm><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></p:xfrm>
        <a:graphic><a:graphicData uri="table"><a:tbl>
          <a:tblPr/><a:tblGrid><a:gridCol w="5"/><a:gridCol w="6"/></a:tblGrid>
          <a:tr h="7"><a:tc><a:tcPr/><a:txBody><a:bodyPr/><a:p><a:r><a:t>X</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:tcPr/><a:txBody><a:bodyPr/><a:p><a:r><a:t>X2</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
          <a:tr h="8"><a:tc><a:tcPr/><a:txBody><a:bodyPr/><a:p><a:r><a:t>Y</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:tcPr/><a:txBody><a:bodyPr/><a:p><a:r><a:t>Y2</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
        </a:tbl></a:graphicData></a:graphic>
      </p:graphicFrame>
      <p:pic>
        <p:nvPicPr><p:cNvPr id="4" name="P1"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>
        <p:blipFill>
          <a:blip r:embed="rId5"/>
          <a:stretch><a:fillRect/></a:stretch>
        </p:blipFill>
        <p:spPr><a:xfrm><a:off x="1" y="2"/><a:ext cx="3" cy="4"/></a:xfrm></p:spPr>
      </p:pic>
    </p:spTree>
  </p:cSld>
</p:sld>"#;

    #[test]
    fn find_ranges() {
        let events = read_events(SLIDE.as_bytes()).unwrap();
        let (s, e) = find_shape_range(&events, 0).unwrap();
        assert!(matches!(&events[s], Event::Start(ev) if ev.name().as_ref() == b"p:sp"));
        let tx = find_txbody_range(&events, s, e).unwrap();
        assert!(matches!(&events[tx.0], Event::Start(ev) if ev.name().as_ref() == b"p:txBody"));
        println!("shape={s}..={e} txbody={tx:?}");
    }

    #[test]
    fn background_inserted_when_absent() {
        let out = set_slide_background(SLIDE.as_bytes(), "FF0000").unwrap();
        let parsed = parse_slide_background(&out);
        assert_eq!(parsed["fill"]["type"], "SOLID");
        assert_eq!(parsed["fill"]["color"], "FF0000");
        let out_str = String::from_utf8(out).unwrap();
        let bg = out_str
            .find("<p:bg>")
            .map(|i| &out_str[i..i + 100])
            .expect("p:bg must be inserted");
        assert!(
            bg.contains(
                "<p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></p:bgPr></p:bg>"
            ),
            "got: {bg}"
        );
        assert!(out_str.contains("<p:spTree>"), "spTree preserved");
    }

    #[test]
    fn background_replaced_when_present() {
        let first = set_slide_background(SLIDE.as_bytes(), "FF0000").unwrap();
        let second = set_slide_background(&first, "0000FF").unwrap();
        let parsed = parse_slide_background(&second);
        assert_eq!(parsed["fill"]["color"], "0000FF");
        let out_str = String::from_utf8(second).unwrap();
        let bg = out_str
            .find("<p:bg>")
            .map(|i| &out_str[i..out_str.find("</p:bg>").unwrap() + 7])
            .unwrap();
        assert_eq!(bg.matches("a:srgbClr").count(), 1, "only one fill in bg");
        assert!(bg.contains("val=\"0000FF\""));
    }

    #[test]
    fn background_none_when_missing() {
        let parsed = parse_slide_background(SLIDE.as_bytes());
        assert!(parsed["fill"]["type"].is_null());
    }

    #[test]
    fn font_size_lossless_preserves_gradient() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("text_frame.paragraphs[0].runs[0].font.size").unwrap();
        let out = replace_shape_property_lossless(xml, 0, &path, "2400").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("gradFill"), "gradient must be preserved");
        assert!(
            out_str.contains("sz=\"2400\"") || out_str.contains("sz=\"2400\""),
            "font size updated"
        );
        assert!(out_str.contains("Hello"), "text preserved");
    }

    #[test]
    fn font_name_creates_latin() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("text_frame.paragraphs[0].runs[0].font.name").unwrap();
        let out = replace_shape_property_lossless(xml, 0, &path, "Arial").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("typeface=\"Arial\""));
        assert!(out_str.contains("gradFill"));
    }

    #[test]
    fn font_color_expands_self_closing_rpr() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("text_frame.paragraphs[0].runs[0].font.color").unwrap();
        let out = replace_shape_property_lossless(xml, 0, &path, "FF0000").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        let snippet = out_str
            .find("<a:rPr lang=\"en-US\" sz=\"1200\">")
            .map(|i| &out_str[i..i + 90])
            .expect("rPr must be expanded into a container");
        assert!(
            snippet.contains("<a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill></a:rPr>"),
            "solidFill must sit inside the rPr, got: {snippet}"
        );
    }

    #[test]
    fn table_cell_text_shorthand() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("table.rows[0].cells[0].text").unwrap();
        let out = replace_table_cell_property_lossless(xml, 1, &path, "Zed").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains(">Zed</a:t>"), "cell text updated");
        assert!(out_str.contains("Y"), "other cell preserved");
    }

    #[test]
    fn alignment_edit() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("text_frame.paragraphs[0].alignment").unwrap();
        let out = replace_shape_property_lossless(xml, 0, &path, "ctr").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("algn=\"ctr\""));
        assert!(out_str.contains("gradFill"));
    }

    #[test]
    fn table_cell_edit() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("table.rows[0].cells[0].text_frame.paragraphs[0].runs[0].text")
            .unwrap();
        let out = replace_table_cell_property_lossless(xml, 1, &path, "Edited").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("Edited"));
    }

    #[test]
    fn run_text_edit() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("text_frame.paragraphs[0].runs[0].text").unwrap();
        let out = replace_shape_property_lossless(xml, 0, &path, "Hi").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains(">Hi<"));
        assert!(out_str.contains("gradFill"));
    }

    #[test]
    fn cell_font_edit() {
        let xml = SLIDE.as_bytes();
        let path =
            path::parse_path("table.rows[0].cells[0].text_frame.paragraphs[0].runs[0].font.size")
                .unwrap();
        let out = replace_table_cell_property_lossless(xml, 1, &path, "2400").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("sz=\"2400\""));
    }

    #[test]
    fn picture_crop_inserts_src_rect() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("crop.left").unwrap();
        let out = replace_picture_crop(xml, 2, &path, "0.25").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(
            out_str.contains("<a:srcRect l=\"25000\""),
            "srcRect with l=25000 inserted, got: {out_str}"
        );
        assert!(out_str.contains("<a:stretch>"), "blipFill preserved");
        assert!(out_str.contains("Hello"), "other shapes preserved");
    }

    #[test]
    fn picture_crop_updates_existing_src_rect() {
        let mut xml = SLIDE.to_string();
        xml = xml.replace(
            "<a:blip r:embed=\"rId5\"/>",
            "<a:blip r:embed=\"rId5\"><a:srcRect l=\"10000\" t=\"5000\"/></a:blip>",
        );
        let path = path::parse_path("crop.top").unwrap();
        let out = replace_picture_crop(xml.as_bytes(), 2, &path, "0.1").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(
            out_str.contains("<a:srcRect l=\"10000\" t=\"10000\""),
            "existing srcRect updated, got: {out_str}"
        );
    }

    #[test]
    fn line_spacing_edit() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("text_frame.paragraphs[0].line_spacing").unwrap();
        let out = replace_shape_property_lossless(xml, 0, &path, "1.5").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("a:spcPct"));
        assert!(out_str.contains("150000"));
        assert!(out_str.contains("gradFill"));
    }

    #[test]
    fn word_wrap_edit() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("text_frame.word_wrap").unwrap();
        let out = replace_shape_property_lossless(xml, 0, &path, "false").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("wrap=\"none\""));
        assert!(out_str.contains("gradFill"));
    }

    #[test]
    fn auto_size_edit() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("text_frame.auto_size").unwrap();
        let out = replace_shape_property_lossless(xml, 0, &path, "shape_to_fit_text").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("a:normAutofit"));
    }

    #[test]
    fn second_table_cell_edit() {
        let xml = SLIDE.as_bytes();
        let path = path::parse_path("table.rows[1].cells[0].text_frame.paragraphs[0].runs[0].text")
            .unwrap();
        let out = replace_table_cell_property_lossless(xml, 1, &path, "Second").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("Second"));
    }

    const CHART: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
              xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
              xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <c:chart>
    <c:autoTitleDeleted val="1"/>
    <c:plotArea>
      <c:layout/>
      <c:barChart>
        <c:barDir val="col"/>
        <c:ser>
          <c:idx val="0"/>
          <c:tx><c:strRef><c:f>Sheet1!$B$1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Series 1</c:v></c:pt></c:strCache></c:strRef></c:tx>
          <c:cat><c:strRef><c:f>Sheet1!$A$2:$A$3</c:f><c:strCache><c:ptCount val="2"/><c:pt idx="0"><c:v>North</c:v></c:pt><c:pt idx="1"><c:v>South</c:v></c:pt></c:strCache></c:strRef></c:cat>
          <c:val><c:numRef><c:f>Sheet1!$B$2:$B$3</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="2"/><c:pt idx="0"><c:v>20</c:v></c:pt><c:pt idx="1"><c:v>50</c:v></c:pt></c:numCache></c:numRef></c:val>
        </c:ser>
      </c:barChart>
    </c:plotArea>
  </c:chart>
</c:chartSpace>"#;

    #[test]
    fn chart_series_name() {
        let path = path::parse_path("chart.series[0].name").unwrap();
        let out = replace_chart_property_lossless(CHART.as_bytes(), &path, "Revenue").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains(">Revenue<"));
        assert!(out_str.contains("Sheet1!$B$1"));
        assert!(out_str.contains("c:barDir val=\"col\""));
    }

    #[test]
    fn chart_category_edit() {
        let path = path::parse_path("chart.series[0].categories[1]").unwrap();
        let out = replace_chart_property_lossless(CHART.as_bytes(), &path, "West").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains(">West<"));
        assert!(out_str.contains(">North<"));
        assert!(out_str.contains(">20</c:v>"));
    }

    #[test]
    fn chart_value_edit() {
        let path = path::parse_path("chart.series[0].values[0]").unwrap();
        let out = replace_chart_property_lossless(CHART.as_bytes(), &path, "99").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains(">99</c:v>"));
        assert!(out_str.contains(">50</c:v>"));
        assert!(out_str.contains(">North</c:v>"));
    }

    #[test]
    fn chart_type_rename() {
        let path = path::parse_path("chart.chart_type").unwrap();
        let out = replace_chart_property_lossless(CHART.as_bytes(), &path, "pieChart").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("<c:pieChart>"));
        assert!(out_str.contains("</c:pieChart>"));
        assert!(out_str.contains("c:ser"));
    }

    #[test]
    fn chart_category_entity_replaced() {
        let xml = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart><c:plotArea><c:lineChart><c:ser><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>S. &amp; Cent.</c:v></c:pt></c:strCache></c:strRef></c:cat></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let path = path::parse_path("chart.series[0].categories[0]").unwrap();
        let out = replace_chart_property_lossless(xml.as_bytes(), &path, "West").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("<c:v>West</c:v>"), "got: {out_str}");
        assert!(!out_str.contains("amp;"));
    }

    #[test]
    fn run_text_entity_replaced() {
        let xml = r#"<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>A &amp; B</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
        let path = path::parse_path("text_frame.paragraphs[0].runs[0].text").unwrap();
        let out = replace_shape_property_lossless(xml.as_bytes(), 0, &path, "C").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("<a:t>C</a:t>"), "got: {out_str}");
        assert!(!out_str.contains("amp;"));
    }

    #[test]
    fn core_property_creates_element_when_absent() {
        let xml = r#"<cp:coreProperties xmlns:cp="x" xmlns:dc="y"><dc:title/></cp:coreProperties>"#;
        let out = replace_core_property(xml.as_bytes(), "author", "alice").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(
            out_str.contains("<dc:creator>alice</dc:creator>"),
            "got: {out_str}"
        );
    }

    #[test]
    fn core_property_expands_self_closing_element() {
        let xml = r#"<cp:coreProperties xmlns:cp="x" xmlns:dc="y"><dc:title/></cp:coreProperties>"#;
        let out = replace_core_property(xml.as_bytes(), "title", "My Deck").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(
            out_str.contains("<dc:title>My Deck</dc:title>"),
            "got: {out_str}"
        );
    }

    #[test]
    fn core_property_replaces_existing_text() {
        let xml = r#"<cp:coreProperties xmlns:cp="x" xmlns:dcterms="z" xmlns:xsi="y"><dcterms:created xsi:type="dcterms:W3CDTF">2020-01-01T00:00:00Z</dcterms:created></cp:coreProperties>"#;
        let out = replace_core_property(xml.as_bytes(), "created", "2024-05-01T10:30:00Z").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(
            out_str.contains("<dcterms:created xsi:type=\"dcterms:W3CDTF\">2024-05-01T10:30:00Z</dcterms:created>"),
            "got: {out_str}"
        );
    }

    #[test]
    fn theme_color_replaced() {
        let xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="Office"><a:accent1><a:srgbClr val="4F81BD"/></a:accent1></a:clrScheme></a:themeElements></a:theme>"#;
        let path = path::parse_path("theme.colors.accent1").unwrap();
        let out = replace_theme_property(xml.as_bytes(), &path[1..], "FF0000").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(
            out_str.contains("<a:srgbClr val=\"FF0000\"/>"),
            "got: {out_str}"
        );
    }

    #[test]
    fn theme_color_created_when_srgb_absent() {
        let xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="Office"><a:accent1><a:sysClr val="windowText"/></a:accent1></a:clrScheme></a:themeElements></a:theme>"#;
        let path = path::parse_path("theme.colors.accent1").unwrap();
        let out = replace_theme_property(xml.as_bytes(), &path[1..], "00FF00").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(
            out_str.contains("<a:srgbClr val=\"00FF00\"/>"),
            "got: {out_str}"
        );
    }

    #[test]
    fn theme_font_replaced() {
        let xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>"#;
        let path = path::parse_path("theme.fonts.major").unwrap();
        let out = replace_theme_property(xml.as_bytes(), &path[1..], "Arial").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("typeface=\"Arial\""), "got: {out_str}");
    }

    #[test]
    fn theme_font_created_when_latin_absent() {
        let xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme name="Office"><a:minorFont/></a:fontScheme></a:themeElements></a:theme>"#;
        let path = path::parse_path("theme.fonts.minor").unwrap();
        let out = replace_theme_property(xml.as_bytes(), &path[1..], "Consolas").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("typeface=\"Consolas\""), "got: {out_str}");
    }

    #[test]
    fn theme_rejects_bad_path() {
        let xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"/>"#;
        let path = path::parse_path("theme.nope.accent1").unwrap();
        assert!(replace_theme_property(xml.as_bytes(), &path[1..], "000000").is_err());
    }

    #[test]
    fn add_chart_series_appends() {
        let path = path::parse_path("chart.series").unwrap();
        let out = add_chart_series_lossless(
            CHART.as_bytes(),
            &path,
            r#"{"name":"S2","categories":["North","South"],"values":[7,8]}"#,
        )
        .unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(out_str.matches("<c:ser>").count(), 2, "got: {out_str}");
        assert!(out_str.contains(">S2<"), "got: {out_str}");
        assert!(out_str.contains(">7</c:v>"), "got: {out_str}");
        // Existing series unchanged.
        assert!(out_str.contains(">Series 1<"), "got: {out_str}");
    }

    #[test]
    fn add_chart_series_inserts_after_index() {
        let path = path::parse_path("chart.series[0]").unwrap();
        let out =
            add_chart_series_lossless(CHART.as_bytes(), &path, r#"{"name":"S2","values":[1]}"#)
                .unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(out_str.matches("<c:ser>").count(), 2, "got: {out_str}");
    }

    #[test]
    fn remove_chart_series() {
        let path = path::parse_path("chart.series[0]").unwrap();
        let out = remove_chart_series_lossless(CHART.as_bytes(), &path).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(out_str.matches("<c:ser>").count(), 0, "got: {out_str}");
        assert!(!out_str.contains("Series 1"), "got: {out_str}");
    }

    #[test]
    fn add_table_row_appends() {
        let path = path::parse_path("table.rows").unwrap();
        let out = add_table_row_lossless(
            SLIDE.as_bytes(),
            1,
            &path,
            r#"{"height":9,"cells":[{},{}]}"#,
        )
        .unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(out_str.matches("<a:tr").count(), 3, "got: {out_str}");
        assert!(out_str.contains("h=\"9\""), "got: {out_str}");
    }

    #[test]
    fn add_table_column_adds_cell_to_each_row() {
        let path = path::parse_path("table.grid").unwrap();
        let out = add_table_column_lossless(SLIDE.as_bytes(), 1, &path, r#"{"width":7}"#).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(out_str.matches("<a:gridCol").count(), 3, "got: {out_str}");
        // Both rows now have 3 cells each.
        assert_eq!(out_str.matches("<a:tc>").count(), 6, "got: {out_str}");
    }

    #[test]
    fn remove_table_row() {
        let path = path::parse_path("table.rows[0]").unwrap();
        let out = remove_table_row_lossless(SLIDE.as_bytes(), 1, &path).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(out_str.matches("<a:tr").count(), 1, "got: {out_str}");
        assert!(!out_str.contains(">X<"), "got: {out_str}");
        assert!(out_str.contains(">Y<"), "got: {out_str}");
    }

    #[test]
    fn remove_table_column_removes_grid_and_cells() {
        let path = path::parse_path("table.grid[1]").unwrap();
        let out = remove_table_column_lossless(SLIDE.as_bytes(), 1, &path)
            .unwrap_or_else(|e| panic!("remove failed: {e:?}"));
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(out_str.matches("<a:gridCol").count(), 1, "got: {out_str}");
        assert_eq!(out_str.matches("<a:tc>").count(), 2, "got: {out_str}");
    }

    #[test]
    fn fill_solid_replaces_gradient_and_preserves_rest() {
        let path = path::parse_path("fill.color.rgb").unwrap();
        let out = replace_shape_fill_lossless(SLIDE.as_bytes(), 0, &path, "0000FF").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(!out_str.contains("gradFill"), "got: {out_str}");
        assert!(
            out_str.contains("<a:solidFill><a:srgbClr val=\"0000FF\"/></a:solidFill>"),
            "got: {out_str}"
        );
        assert!(out_str.contains("<a:xfrm>"), "xfrm preserved");
        assert!(out_str.contains("Hello"), "text preserved");
    }

    #[test]
    fn fill_type_nofill() {
        let path = path::parse_path("fill.type").unwrap();
        let out = replace_shape_fill_lossless(SLIDE.as_bytes(), 0, &path, "no_fill").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(!out_str.contains("gradFill"), "got: {out_str}");
        assert!(out_str.contains("<a:noFill/>"), "got: {out_str}");
        assert!(out_str.contains("Hello"), "text preserved");
    }

    #[test]
    fn fill_type_nofill_alias() {
        let path = path::parse_path("fill.type").unwrap();
        let out = replace_shape_fill_lossless(SLIDE.as_bytes(), 0, &path, "nofill").unwrap();
        assert!(String::from_utf8(out).unwrap().contains("<a:noFill/>"));
    }

    #[test]
    fn fill_theme_color_uses_scheme() {
        let path = path::parse_path("fill.color.theme_color").unwrap();
        let out = replace_shape_fill_lossless(SLIDE.as_bytes(), 0, &path, "accent1").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(
            out_str.contains("<a:solidFill><a:schemeClr val=\"accent1\"/></a:solidFill>"),
            "got: {out_str}"
        );
    }

    #[test]
    fn fill_inserter_into_picture_sppr() {
        let path = path::parse_path("fill.color.rgb").unwrap();
        let out = replace_shape_fill_lossless(SLIDE.as_bytes(), 2, &path, "FF00AA").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(
            out_str.contains("<a:solidFill><a:srgbClr val=\"FF00AA\"/></a:solidFill>"),
            "got: {out_str}"
        );
        assert!(out_str.contains("r:embed=\"rId5\""), "blipFill preserved");
    }

    #[test]
    fn outline_creates_ln_with_attrs_and_fill() {
        let path = path::parse_path("outline").unwrap();
        let out = replace_shape_outline_lossless(
            SLIDE.as_bytes(),
            0,
            &path,
            r#"{"width":9525,"dash":"solid","fill":{"type":"solid","color":{"rgb":"000000"}}}"#,
        )
        .unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("a:ln w=\"9525\""), "got: {out_str}");
        assert!(
            out_str.contains("<a:prstDash val=\"solid\"/>"),
            "got: {out_str}"
        );
        assert!(
            out_str.contains("<a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill>"),
            "got: {out_str}"
        );
        assert!(out_str.contains("gradFill"), "shape fill preserved");
        assert!(out_str.contains("Hello"), "text preserved");
    }

    #[test]
    fn outline_leaf_width() {
        let path = path::parse_path("outline.width").unwrap();
        let out = replace_shape_outline_lossless(SLIDE.as_bytes(), 0, &path, "12700").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("a:ln w=\"12700\""), "got: {out_str}");
        assert!(out_str.contains("gradFill"), "shape fill preserved");
    }

    #[test]
    fn outline_replace_existing_ln() {
        let path = path::parse_path("outline.width").unwrap();
        let once = replace_shape_outline_lossless(SLIDE.as_bytes(), 0, &path, "9525").unwrap();
        let twice = replace_shape_outline_lossless(&once, 0, &path, "12700").unwrap();
        let out_str = String::from_utf8(twice).unwrap();
        assert_eq!(out_str.matches("<a:ln").count(), 1, "got: {out_str}");
        assert!(out_str.contains("w=\"12700\""), "got: {out_str}");
        assert!(!out_str.contains("w=\"9525\""), "got: {out_str}");
    }

    #[test]
    fn text_frame_whole_json_rich_content() {
        let path = path::parse_path("text_frame").unwrap();
        let value = r#"{"paragraphs":[{"runs":[{"text":"Hello","font":{"size":2400,"bold":true}}]},{"alignment":"CENTER","runs":[{"text":"World","font":{"italic":true}}]}]}"#;
        let out = replace_shape_property_lossless(SLIDE.as_bytes(), 0, &path, value).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains(">Hello</a:t>"), "got: {out_str}");
        assert!(out_str.contains(">World</a:t>"), "got: {out_str}");
        assert!(
            out_str.contains("sz=\"2400\""),
            "per-run size, got: {out_str}"
        );
        assert!(out_str.contains("b=\"1\""), "per-run bold, got: {out_str}");
        assert!(
            out_str.contains("i=\"1\""),
            "per-run italic, got: {out_str}"
        );
        assert!(
            out_str.contains("algn=\"ctr\""),
            "paragraph alignment, got: {out_str}"
        );
        assert!(out_str.contains("gradFill"), "spPr preserved");
        assert!(out_str.contains("id=\"2\""), "shape identity preserved");
    }

    #[test]
    fn table_cell_text_frame_whole_json() {
        let path = path::parse_path("table.rows[0].cells[0].text_frame").unwrap();
        let value = r#"{"paragraphs":[{"runs":[{"text":"NewCell","font":{"size":1800}}]}]}"#;
        let out = replace_table_cell_property_lossless(SLIDE.as_bytes(), 1, &path, value).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains("NewCell"), "got: {out_str}");
        assert!(out_str.contains("sz=\"1800\""), "got: {out_str}");
        assert!(out_str.contains(">Y<"), "other cell preserved");
    }

    #[test]
    fn text_frame_json_rejects_non_object() {
        let path = path::parse_path("text_frame").unwrap();
        assert!(replace_shape_property_lossless(SLIDE.as_bytes(), 0, &path, "hello").is_err());
    }
}
