use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::escape::escape;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

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

fn find_elem_range(events: &[Event<'_>], name: &[u8], start_from: usize) -> Option<(usize, usize)> {
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

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;

    edit_txbody_path(&mut events, txbody_start, txbody_end, remaining, value)?;
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

                let child_name_owned = String::from_utf8_lossy(child_name).to_string();
                events.insert(s + 1, Event::Empty(BytesStart::new(child_name_owned)));
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

    edit_txbody_path(&mut events, txbody_start, txbody_end, rest, value)?;
    write_events(events)
}

fn is_chart_type_tag(name: &[u8]) -> bool {
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
            let mut latin = BytesStart::new("a:latin");
            latin.push_attribute(("typeface", value));
            events.insert(s + 1, Event::Empty(latin));
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
        let insert_at = s + 1;
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
            let mut latin = BytesStart::new("a:latin");
            latin.push_attribute(("typeface", value));
            events.insert(s + 1, Event::Empty(latin));
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
        let color_events = color_xml_events(value);
        for (j, ev) in color_events.into_iter().enumerate() {
            events.insert(s + 1 + j, ev);
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
          <a:tr h="7"><a:tc><a:tcPr/><a:txBody><a:bodyPr/><a:p><a:r><a:t>X</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
          <a:tr h="8"><a:tc><a:tcPr/><a:txBody><a:bodyPr/><a:p><a:r><a:t>Y</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
        </a:tbl></a:graphicData></a:graphic>
      </p:graphicFrame>
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
}
