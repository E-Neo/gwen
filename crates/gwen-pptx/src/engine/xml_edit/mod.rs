use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::escape::escape;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use std::io::Write;

use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::path;

mod chart;
mod core;
mod shape;
mod table;
mod text;

pub use shape::{
    delete_picture_crop, delete_picture_crop_side, delete_shape_fill, delete_shape_outline,
    delete_shape_text_frame, replace_or_create_text_frame_lossless, replace_picture_crop,
    replace_shape_fill_lossless, replace_shape_outline_lossless, replace_shape_property_lossless,
    replace_shape_text_lossless, set_auto_shape_type_lossless,
};

pub use text::{
    delete_para_font_lossless, delete_para_prop, delete_run_font_lossless, delete_txbody_prop,
    replace_lst_style_lossless, replace_para_font_lossless, replace_paragraph_lossless,
    replace_paragraph_runs_lossless, replace_run_font_lossless, replace_run_lossless,
};

pub use table::{
    add_table_column_lossless, add_table_row_lossless, remove_table_column_lossless,
    remove_table_row_lossless, replace_table_cell_lossless, replace_table_cell_property_lossless,
    replace_table_grid_col_lossless, replace_table_row_lossless, replace_whole_table_lossless,
};

pub use chart::{
    add_chart_point_lossless, add_chart_series_lossless, remove_chart_point_lossless,
    remove_chart_series_lossless, replace_chart_property_lossless, replace_chart_series_lossless,
};

pub use core::{
    THEME_COLOR_NAMES, core_prop_key, delete_core_property, parse_slide_background,
    replace_core_property, replace_theme_property, set_slide_background,
};

pub(crate) use text::edit_txbody_path;

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

/// True for any element that opens a shape: a regular shape, picture,
/// connector, group, or graphic frame (table/chart).
pub(crate) fn is_shape_tag(name: &[u8]) -> bool {
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

/// Remove an attribute from an element at `start` (handles both empty and
/// non-empty forms).
fn remove_attr(events: &mut [Event<'static>], start: usize, key: &[u8]) {
    let (Event::Empty(orig) | Event::Start(orig)) = &events[start] else {
        return;
    };
    let name = String::from_utf8_lossy(orig.name().as_ref()).to_string();
    let mut elem = BytesStart::new(name);
    for attr in orig.attributes().flatten() {
        if attr.key.as_ref() != key {
            let ak = String::from_utf8_lossy(attr.key.as_ref()).to_string();
            let av = String::from_utf8_lossy(&attr.value).to_string();
            elem.push_attribute((ak.as_str(), av.as_str()));
        }
    }
    events[start] = match &events[start] {
        Event::Empty(_) => Event::Empty(elem),
        _ => Event::Start(elem),
    };
}

fn remove_attr_from_start(orig: &BytesStart<'_>, key: &[u8]) -> BytesStart<'static> {
    let name = String::from_utf8_lossy(orig.name().as_ref()).to_string();
    let mut elem = BytesStart::new(name);
    for attr in orig.attributes().flatten() {
        if attr.key.as_ref() != key {
            let ak = String::from_utf8_lossy(attr.key.as_ref()).to_string();
            let av = String::from_utf8_lossy(&attr.value).to_string();
            elem.push_attribute((ak.as_str(), av.as_str()));
        }
    }
    elem
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

    #[test]
    fn replace_shape_text_overwrites_existing_text() {
        let out = replace_shape_text_lossless(SLIDE.as_bytes(), 0, "Hi").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(out_str.matches(">Hi</a:t>").count(), 1, "got: {out_str}");
        assert!(!out_str.contains(">Hello</a:t>"), "got: {out_str}");
        // Non-text shape-level content preserved.
        assert!(out_str.contains("gradFill"), "got: {out_str}");
        assert!(out_str.contains("id=\"2\""), "got: {out_str}");
    }

    #[test]
    fn replace_shape_text_creates_text_when_empty() {
        const XML: &str = r#"<p:sp xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
          xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
          <p:nvSpPr><p:cNvPr id="1" name="S"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
          <p:spPr/>
          <p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang="en-US"/></a:p></p:txBody>
        </p:sp>"#;
        let out = replace_shape_text_lossless(XML.as_bytes(), 0, "Hello World").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(
            out_str.matches(">Hello World</a:t>").count(),
            1,
            "got: {out_str}"
        );
        // The run must precede the end-paragraph run keep mark.
        let before_endpara = out_str.split("<a:endParaRPr").next().unwrap();
        assert!(
            before_endpara.contains("<a:t>Hello World</a:t>"),
            "got: {out_str}"
        );
    }

    #[test]
    fn replace_shape_text_collapses_multiple_paragraphs() {
        const XML: &str = r#"<p:sp xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
          xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
          <p:nvSpPr><p:cNvPr id="1" name="S"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr>
          <p:spPr/>
          <p:txBody>
            <a:bodyPr/>
            <a:p><a:r><a:t>部门：</a:t></a:r></a:p>
            <a:p><a:r><a:t>作者：</a:t></a:r></a:p>
            <a:p><a:r><a:t>日期：</a:t></a:r></a:p>
          </p:txBody>
        </p:sp>"#;
        let out = replace_shape_text_lossless(XML.as_bytes(), 0, "Hello World").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert_eq!(
            out_str.matches(">Hello World</a:t>").count(),
            1,
            "expected one collapsed run, got: {out_str}"
        );
        assert_eq!(out_str.matches("<a:p>").count(), 1, "got: {out_str}");
    }

    #[test]
    fn replace_shape_text_rejects_shape_without_text_frame() {
        // shape_idx 2 is the picture (p:pic) with no txBody.
        assert!(replace_shape_text_lossless(SLIDE.as_bytes(), 2, "Hi").is_err());
    }

    #[test]
    fn table_cell_paragraph_whole_runs_replace() {
        // A paragraph that gained an entire run array (e.g. an empty cell
        // paragraph that got text) must replace its `a:r` children in place.
        let path =
            path::parse_path("table.rows[0].cells[0].text_frame.paragraphs[0].runs").unwrap();
        let value = r#"[{"text":"New","font":{"size":1800,"bold":true}},{"text":" Runs"}]"#;
        let out = replace_table_cell_property_lossless(SLIDE.as_bytes(), 1, &path, value).unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(out_str.contains(">New</a:t>"), "got: {out_str}");
        assert!(out_str.contains("sz=\"1800\""), "got: {out_str}");
        assert!(out_str.contains("b=\"1\""), "got: {out_str}");
        assert!(out_str.contains("> Runs</a:t>"), "got: {out_str}");
        assert!(!out_str.contains(">X</a:t>"), "old run replaced: {out_str}");
        assert!(out_str.contains(">X2<"), "sibling cell preserved");
    }

    #[test]
    fn table_cell_paragraph_whole_runs_clear() {
        // Clearing a paragraph's runs keeps the paragraph (and its pPr) but
        // removes the runs — the `[]` deletion path.
        let path =
            path::parse_path("table.rows[0].cells[0].text_frame.paragraphs[0].runs").unwrap();
        let out = replace_table_cell_property_lossless(SLIDE.as_bytes(), 1, &path, "[]").unwrap();
        let out_str = String::from_utf8(out).unwrap();
        assert!(!out_str.contains(">X</a:t>"), "runs cleared: {out_str}");
        assert!(out_str.contains("<a:p>"), "paragraph survives: {out_str}");
    }
}
