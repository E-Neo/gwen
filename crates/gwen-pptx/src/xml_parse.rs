//! Minimal XML parsing helpers shared by the query and model layers.
//!
//! These were extracted from the legacy lossless-edit engine (`engine/xml_edit`)
//! which the project-mirror compiler replaced; only the read-side helpers the
//! mirror still needs live here.

use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::json;

use crate::error::{AppError, AppResult};

pub fn read_events(xml_bytes: &[u8]) -> AppResult<Vec<Event<'static>>> {
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

/// The `[start, end]` index range of the first element named `name` at or after
/// `start_from`, matching its balancing close tag.
pub fn find_elem_range(
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

/// The `[start, end]` range of the first direct child element named `child_name`
/// inside a parent range. For a self-closing element `start == end`.
pub fn find_child_elem_range(
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

pub fn is_chart_type_tag(name: &[u8]) -> bool {
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

/// The ordered theme color names used by `a:clrScheme`.
pub const THEME_COLOR_NAMES: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

/// The slide's background fill, as the mirror's `background` marker expects:
/// `{ "fill": { "type": "SOLID"|"GRADIENT"|"THEME"|null, "color": ... } }`.
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
