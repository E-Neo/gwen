use super::*;

pub(crate) fn edit_txbody_path(
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
                Some((path::PathSegment::Field(n), tail)) if n == "runs" => match tail.first() {
                    None => {
                        // Whole-array replacement of a paragraph's runs (e.g. an
                        // empty cell paragraph that gained text, or a runs
                        // array cleared to empty).
                        let runs: Vec<crate::dto::RunDto> =
                            serde_json::from_str(value).map_err(|e| {
                                AppError::InvalidValue(format!("Invalid runs JSON: {e}"))
                            })?;
                        let inner: String = runs.iter().map(crate::dto::xml::run_to_xml).collect();
                        let run_events = read_events(inner.as_bytes())?;
                        splice_paragraph_runs(
                            events,
                            txbody_start,
                            txbody_end,
                            para_idx,
                            run_events,
                        )
                    }
                    Some(path::PathSegment::Index(run_idx)) => {
                        let run_idx = *run_idx;
                        match tail.get(1) {
                            Some(path::PathSegment::Field(n)) if n == "font" => {
                                edit_run_font_in_place(
                                    events,
                                    txbody_start,
                                    txbody_end,
                                    para_idx,
                                    run_idx,
                                    &tail[2..],
                                    value,
                                )
                            }
                            Some(path::PathSegment::Field(n)) if n == "text" => {
                                edit_run_text_in_place(
                                    events,
                                    txbody_start,
                                    txbody_end,
                                    para_idx,
                                    run_idx,
                                    value,
                                )
                            }
                            _ => Err(AppError::PathParse(
                                "Expected 'font' or 'text' after run index".to_string(),
                            )),
                        }
                    }
                    _ => Err(AppError::PathParse("Expected run index".to_string())),
                },
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

/// Replace the whole `a:lstStyle` of a shape's text frame (the
/// `default_paragraph_style` field) with a freshly serialized level-1 style.
pub fn replace_lst_style_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let dps: crate::dto::ParagraphDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid paragraph style JSON: {e}")))?;
    let inner_events = read_events(crate::dto::xml::lst_style_to_xml(&dps).as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    let lst_start = txbody_start + 1;
    let (lst_range, insert_pos) = if let Some(r) =
        find_elem_range(&events, b"a:lstStyle", txbody_start)
        && r.0 <= txbody_end
    {
        (Some(r), r.0)
    } else {
        (None, lst_start)
    };
    if let Some((s, e)) = lst_range {
        events.splice(s..=e, inner_events);
    } else {
        for ev in inner_events.into_iter().rev() {
            events.insert(insert_pos, ev);
        }
    }
    write_events(events)
}

/// Replace the whole paragraph at `text_frame.paragraphs[k]` with a freshly
/// serialized `ParagraphDto`, splicing the XML element in place.
pub fn replace_paragraph_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    para_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let para: crate::dto::ParagraphDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid paragraph JSON: {e}")))?;
    let mut writer = Writer::new(Vec::new());
    crate::dto::xml::write_paragraph(&para, &mut writer);
    let inner_events = read_events(&writer.into_inner())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    let (para_start, para_end) =
        find_nth_child_range(&events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;
    events.splice(para_start..=para_end, inner_events);
    write_events(events)
}

/// Replace a paragraph's runs with a freshly serialized array of `RunDto`,
/// preserving the paragraph's `a:pPr` and `a:endParaRPr`.
pub fn replace_paragraph_runs_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    para_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let runs: Vec<crate::dto::RunDto> = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid runs JSON: {e}")))?;
    let inner: String = runs.iter().map(crate::dto::xml::run_to_xml).collect();
    let run_events = read_events(inner.as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    splice_paragraph_runs(&mut events, txbody_start, txbody_end, para_idx, run_events)?;
    write_events(events)
}

/// Replace the `a:r` children of the paragraph at `para_idx` inside the located
/// text body range with `run_events`, preserving the paragraph's `a:pPr` and
/// `a:endParaRPr`.
fn splice_paragraph_runs(
    events: &mut Vec<Event<'static>>,
    txbody_start: usize,
    txbody_end: usize,
    para_idx: usize,
    run_events: Vec<Event<'static>>,
) -> AppResult<()> {
    let (para_start, para_end) =
        find_nth_child_range(events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;

    let mut out = Vec::new();
    out.extend_from_slice(&events[..para_start + 1]);
    let mut i = para_start + 1;
    while i < para_end {
        if let Event::Start(e) = &events[i] {
            let name = e.name().as_ref().to_vec();
            if name.as_slice() == b"a:r" {
                let r = find_elem_range(events, &name, i);
                if let Some((_, end)) = r {
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(events[i].clone());
        i += 1;
    }
    out.extend(run_events);
    out.extend_from_slice(&events[para_end..]);
    *events = out;
    Ok(())
}

/// Replace the whole run at `text_frame.paragraphs[k].runs[m]` with a freshly
/// serialized `RunDto`.
pub fn replace_run_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    para_idx: usize,
    run_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let run: crate::dto::RunDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid run JSON: {e}")))?;
    let inner_events = read_events(crate::dto::xml::run_to_xml(&run).as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    let (para_start, para_end) =
        find_nth_child_range(&events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;
    let (run_start, run_end) = find_nth_child_range(&events, para_start, para_end, b"a:r", run_idx)
        .ok_or_else(|| AppError::PathParse(format!("Run index {run_idx} not found")))?;
    events.splice(run_start..=run_end, inner_events);
    write_events(events)
}

/// Replace the `a:rPr` of a run with a freshly serialized `FontDto`, creating
/// the property element when the run has none.
pub fn replace_run_font_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    para_idx: usize,
    run_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let font: crate::dto::FontDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid font JSON: {e}")))?;
    let inner_events = read_events(crate::dto::xml::rpr_to_xml(&font).as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    let (para_start, para_end) =
        find_nth_child_range(&events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;
    let (run_start, run_end) = find_nth_child_range(&events, para_start, para_end, b"a:r", run_idx)
        .ok_or_else(|| AppError::PathParse(format!("Run index {run_idx} not found")))?;

    let (rpr_range, insert_pos) = find_rpr_in_run(&events, run_start, run_end);
    if let Some((s, e)) = rpr_range {
        events.splice(s..=e, inner_events);
    } else {
        for ev in inner_events.into_iter().rev() {
            events.insert(insert_pos, ev);
        }
    }
    write_events(events)
}

/// Replace the `a:endParaRPr` of a paragraph with a freshly serialized
/// `FontDto`, creating it when absent.
pub fn replace_para_font_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    para_idx: usize,
    value: &str,
) -> AppResult<Vec<u8>> {
    let font: crate::dto::FontDto = serde_json::from_str(value)
        .map_err(|e| AppError::InvalidValue(format!("Invalid font JSON: {e}")))?;
    let inner_events = read_events(crate::dto::xml::end_para_rpr_to_xml(&font).as_bytes())?;

    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    let (para_start, para_end) =
        find_nth_child_range(&events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;

    let end_rpr = find_elem_range(&events, b"a:endParaRPr", para_start).filter(|r| r.0 <= para_end);
    match end_rpr {
        Some((s, e)) => {
            events.splice(s..=e, inner_events);
        }
        None => {
            for ev in inner_events.into_iter().rev() {
                events.insert(para_end, ev);
            }
        }
    }
    write_events(events)
}

/// Remove a text frame body property (a field set to `null`). For attribute
/// props the attribute is dropped; for `auto_size` the autofit child is removed.
pub fn delete_txbody_prop(xml_bytes: &[u8], shape_idx: usize, prop: &str) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    let bodypr_range =
        find_elem_range(&events, b"a:bodyPr", txbody_start).filter(|r| r.0 <= txbody_end);

    let Some((s, e)) = bodypr_range else {
        return Ok(xml_bytes.to_vec());
    };
    match prop {
        "auto_size" => {
            let mut to_remove: Vec<usize> = Vec::new();
            let mut j = s + 1;
            while j < e {
                let should = matches!(
                    &events[j],
                    Event::Start(ev) | Event::Empty(ev)
                        if matches!(ev.name().as_ref(), b"a:spAutoFit" | b"a:normAutofit" | b"a:noAutofit")
                );
                if should {
                    to_remove.push(j);
                }
                j += 1;
            }
            for idx in to_remove.into_iter().rev() {
                events.remove(idx);
            }
        }
        "word_wrap" | "vertical_anchor" | "margin_left" | "margin_right" | "margin_top"
        | "margin_bottom" => {
            let attr_key: &[u8] = match prop {
                "word_wrap" => b"wrap",
                "vertical_anchor" => b"anchor",
                "margin_left" => b"lIns",
                "margin_right" => b"rIns",
                "margin_top" => b"tIns",
                "margin_bottom" => b"bIns",
                _ => unreachable!(),
            };
            remove_attr(&mut events, s, attr_key);
        }
        _ => {
            return Err(AppError::PathParse(format!(
                "Unknown text frame property '{prop}'"
            )));
        }
    }
    write_events(events)
}

/// Remove a paragraph property (a field set to `null`): drops the `algn`/`lvl`
/// attribute or the spacing child element.
pub fn delete_para_prop(
    xml_bytes: &[u8],
    shape_idx: usize,
    para_idx: usize,
    prop: &str,
) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    let (para_start, para_end) =
        find_nth_child_range(&events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;

    match prop {
        "alignment" | "level" => {
            let attr_key: &[u8] = if prop == "alignment" { b"algn" } else { b"lvl" };
            let (ppr_range, _) = find_ppr_in_para(&events, para_start, para_end);
            if let Some((s, _e)) = ppr_range {
                remove_attr(&mut events, s, attr_key);
            }
        }
        "line_spacing" | "space_before" | "space_after" => {
            let tag: &[u8] = match prop {
                "line_spacing" => b"a:lnSpc",
                "space_before" => b"a:spcBef",
                _ => b"a:spcAft",
            };
            if let Some((s, e)) =
                find_elem_range(&events, tag, para_start).filter(|r| r.0 <= para_end)
            {
                for j in (s..=e).rev() {
                    events.remove(j);
                }
            }
        }
        _ => {
            return Err(AppError::PathParse(format!(
                "Unknown paragraph property '{prop}'"
            )));
        }
    }
    write_events(events)
}

/// Remove the `a:rPr` from a run (a `null` font in the overlay).
pub fn delete_run_font_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    para_idx: usize,
    run_idx: usize,
) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    let (para_start, para_end) =
        find_nth_child_range(&events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;
    let (run_start, run_end) = find_nth_child_range(&events, para_start, para_end, b"a:r", run_idx)
        .ok_or_else(|| AppError::PathParse(format!("Run index {run_idx} not found")))?;
    if let Some((s, e)) = find_elem_range(&events, b"a:rPr", run_start).filter(|r| r.0 <= run_end) {
        for j in (s..=e).rev() {
            events.remove(j);
        }
    }
    write_events(events)
}

/// Remove the `a:endParaRPr` from a paragraph (a `null` font).
pub fn delete_para_font_lossless(
    xml_bytes: &[u8],
    shape_idx: usize,
    para_idx: usize,
) -> AppResult<Vec<u8>> {
    let mut events = read_events(xml_bytes)?;
    let (shape_start, shape_end) =
        find_shape_range(&events, shape_idx).ok_or(AppError::ShapeIndexOutOfBounds(shape_idx))?;
    let (txbody_start, txbody_end) = find_txbody_range(&events, shape_start, shape_end)
        .ok_or_else(|| AppError::PathParse("Shape has no text frame".to_string()))?;
    let (para_start, para_end) =
        find_nth_child_range(&events, txbody_start, txbody_end, b"a:p", para_idx)
            .ok_or_else(|| AppError::PathParse(format!("Paragraph index {para_idx} not found")))?;
    if let Some((s, e)) =
        find_elem_range(&events, b"a:endParaRPr", para_start).filter(|r| r.0 <= para_end)
    {
        for j in (s..=e).rev() {
            events.remove(j);
        }
    }
    write_events(events)
}
